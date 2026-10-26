# Fetch quantised manga-ocr exports beside the installed model, for measurement.
#
# The app ships the fp32 export (461 MB, from mayocream/manga-ocr-onnx). onnx-community
# publishes the *same base model* at eight precisions, down to 74 MB. Whether any of them
# reads the same text is a question this repository can answer rather than guess:
# `core/tests/quantisation.rs` scores each against the fp32 output over the OCR corpus.
#
#   pwsh -File scripts/fetch-manga-ocr-variants.ps1                  # fp16, int8, q4f16
#   pwsh -File scripts/fetch-manga-ocr-variants.ps1 -Variants q4,bnb4
#
# Nothing here is needed to *use* the app — these are research artifacts, downloaded on
# request and never by the installer.

[CmdletBinding()]
param(
    # Which precisions to fetch. Names are onnx-community's own file suffixes.
    [string[]] $Variants = @('fp16', 'int8', 'q4f16'),
    [string] $ModelRoot = (Join-Path $env:APPDATA 'com.sakatsu.app\models')
)

$ErrorActionPreference = 'Stop'

# `pwsh -File this.ps1 -Variants fp16,int8` binds the whole comma list as ONE string, while
# calling it from inside PowerShell binds three. Splitting here makes both spellings mean the
# same thing, instead of the first one creating a directory named "manga-ocr-fp16,int8".
$Variants = $Variants | ForEach-Object { $_ -split ',' } | Where-Object { $_ -ne '' }

$repo = 'onnx-community/manga-ocr-base-ONNX'
$base = Join-Path $ModelRoot 'manga-ocr'

# The tokenizer and the preprocessing config are not in the variant repo, and they are not
# variant-specific: every export here is the same base model at a different precision. They
# are copied from the installed model rather than re-downloaded, which also means a variant
# can never disagree with the baseline about what token 4173 is.
$shared = @('vocab.txt', 'config.json', 'generation_config.json', 'preprocessor_config.json')

if (-not (Test-Path (Join-Path $base 'vocab.txt'))) {
    throw "No manga-ocr at $base. Run scripts/fetch-ocr-model.ps1 first — the variants borrow its tokenizer."
}

foreach ($variant in $Variants) {
    $target = Join-Path $ModelRoot "manga-ocr-$variant"
    Write-Host "`n=== $variant -> $target ===" -ForegroundColor Cyan
    New-Item -ItemType Directory -Force $target | Out-Null

    foreach ($part in @('encoder', 'decoder')) {
        # Downloaded under the plain name the loader expects, so a variant directory is
        # indistinguishable from the real one to `MangaOcr::load`.
        $out = Join-Path $target "${part}_model.onnx"
        if (Test-Path $out) { Write-Host "  ${part}: already there"; continue }
        $url = "https://huggingface.co/$repo/resolve/main/onnx/${part}_model_${variant}.onnx"
        Write-Host "  ${part}: downloading..."
        try {
            Invoke-WebRequest -Uri $url -OutFile $out -UseBasicParsing
        } catch {
            Remove-Item $out -ErrorAction SilentlyContinue
            throw "$variant/$part failed: $_`nIs '$variant' one of onnx-community's suffixes? (fp16 int8 uint8 q4 q4f16 bnb4 quantized)"
        }
        # A repository that answers a missing file with an HTML error page produces an
        # 'ONNX' file that fails much later and much more confusingly.
        $bytes = [System.IO.File]::ReadAllBytes($out)[0..3]
        if ($bytes[0] -eq 0x3C) { Remove-Item $out; throw "$variant/$part came back as HTML, not ONNX" }
        Write-Host ("    {0:N0} MB" -f ((Get-Item $out).Length / 1MB))
    }

    foreach ($file in $shared) {
        Copy-Item (Join-Path $base $file) (Join-Path $target $file) -Force
    }
}

Write-Host "`nMeasure them:" -ForegroundColor Green
Write-Host "  cargo test -p sakatsu-core --test quantisation -- --ignored --nocapture"
