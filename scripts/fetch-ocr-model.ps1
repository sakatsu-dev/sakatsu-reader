<#
.SYNOPSIS
    Downloads the manga-ocr recognition model into the user profile.

.DESCRIPTION
    屠つ ships by fetch, not by bundle (project notes §6 applies to models as much as to
    dictionaries): this is ~111 MB and is never committed to this repository.

    Nothing here runs automatically. It is a deliberate, user-initiated download.

    manga-ocr is the model project notes §3 chose: it is trained on manga, which is where
    Japanese OCR usually falls over — vertical text, furigana, and hand-lettered
    fonts. These files are an ONNX export of kha-white/manga-ocr-base, so they run
    in-process through ONNX Runtime with no Python involved.

    Licence: manga-ocr is Apache-2.0. As with the dictionaries, that travels with the
    data — see project notes §8.

.PARAMETER Destination
    Where to put the model. Defaults to the app data directory Tauri uses for
    identifier `com.sakatsu.app` on Windows.

.PARAMETER Force
    Re-download even if a file is already there.

.EXAMPLE
    pwsh -File scripts/fetch-ocr-model.ps1
#>
[CmdletBinding()]
param(
    [string] $Destination = (Join-Path $env:APPDATA 'com.sakatsu.app\models\manga-ocr'),
    [switch] $Force
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# Two publishers, and the split is deliberate.
#
# The weights are the quantised uint8 export, which is a quarter of the size of the fp32
# one and 27% faster to read with. `core/tests/ocr_corpus.rs` scored both against 141 real
# captures before this was chosen: 0.1530 mean character error for fp32 against 0.1561 for
# uint8, and uint8 read one *more* sample perfectly.
#
# `onnx-community` publishes no `vocab.txt`, so the four small files still come from
# `mayocream`. That is the same mix `fetch-manga-ocr-variants.ps1` assembles for measuring,
# which is the point: what ships is what was scored.
$repository = 'mayocream/manga-ocr-onnx'
$revision = '24b12778d85800835e2ca409236de281b8ab7b9f'
$base = "https://huggingface.co/$repository/resolve/$revision"

$weightsRepository = 'onnx-community/manga-ocr-base-ONNX'
$weightsRevision = 'f9023406bb2f6b17df67bc4a327c56ecd20611f0'
$weightsBase = "https://huggingface.co/$weightsRepository/resolve/$weightsRevision/onnx"

# `Url` where the file does not come from `$base`, and the local name is the plain one the
# loader expects rather than the publisher's suffixed one.
$files = @(
    @{ Name = 'encoder_model.onnx'; Url = "$weightsBase/encoder_model_uint8.onnx"; About = 'the vision encoder, uint8 (~83 MB)' }
    @{ Name = 'decoder_model.onnx'; Url = "$weightsBase/decoder_model_uint8.onnx"; About = 'the text decoder, uint8 (~28 MB)' }
    # Small, and all of them load-bearing: the vocabulary turns token ids back into
    # Japanese, and the configs carry the exact preprocessing and the token ids the
    # decoder starts and stops on.
    @{ Name = 'vocab.txt'; About = 'the wordpiece vocabulary' }
    @{ Name = 'config.json'; About = 'model configuration' }
    @{ Name = 'generation_config.json'; About = 'decoder start / end token ids' }
    @{ Name = 'preprocessor_config.json'; About = 'image size and normalization' }
)

if (-not (Test-Path $Destination)) {
    New-Item -ItemType Directory -Force -Path $Destination | Out-Null
}
Write-Output "Source     : https://huggingface.co/$repository"
Write-Output "Weights    : https://huggingface.co/$weightsRepository (uint8)"
Write-Output "Destination: $Destination`n"

foreach ($file in $files) {
    $path = Join-Path $Destination $file.Name

    if ((Test-Path $path) -and -not $Force) {
        $size = [math]::Round((Get-Item $path).Length / 1MB, 1)
        Write-Output "$($file.Name): already present (${size} MB) — pass -Force to replace it"
        continue
    }

    Write-Output "$($file.Name): downloading — $($file.About)"
    # Download beside the target and move into place, so an interrupted run cannot
    # leave a truncated model that fails to load for no obvious reason.
    $partial = "$path.partial"
    try {
        $uri = if ($file.Url) { $file.Url } else { "$base/$($file.Name)" }
        Invoke-WebRequest -Uri $uri -OutFile $partial -MaximumRedirection 5
        Move-Item -Path $partial -Destination $path -Force
        $size = [math]::Round((Get-Item $path).Length / 1MB, 1)
        Write-Output "$($file.Name): done (${size} MB)"
    }
    catch {
        if (Test-Path $partial) { Remove-Item $partial -Force }
        Write-Output "$($file.Name): FAILED — $($_.Exception.Message)"
        Write-Output "  If the repository has moved, check huggingface.co rather than guessing a URL."
        continue
    }
}

$total = [math]::Round((Get-ChildItem $Destination -File | Measure-Object -Property Length -Sum).Sum / 1MB, 1)
Write-Output "`nTotal: ${total} MB in $Destination"
Write-Output "Licence: manga-ocr is Apache-2.0 (kha-white/manga-ocr-base); this is an ONNX export of it."
