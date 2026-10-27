<#
.SYNOPSIS
    Downloads the FuguMT Japanese→English translation model.

.DESCRIPTION
    ~310 MB, fetched rather than bundled, and never committed (project notes §6).
    Nothing here runs automatically.

    FuguMT is the model project notes §7 chose: small enough to run offline on a laptop,
    and good enough to check your own reading against. It is about 61M parameters —
    it gets plenty wrong, which is why the UI labels its output a rough gist rather
    than a translation, and why it stays closed until you open it.

    The plain (non-quantized) encoder and decoder are used rather than the int8 ones.
    They are roughly twice the size, but the model is already small enough to make
    mistakes, and quantization would compound them — a wrong translation is worse
    than a larger download.

    `decoder_with_past` is skipped too: the simple decoder re-runs the whole prefix
    each step, which is slower but needs no cache plumbing at all.

    Licence: FuguMT is CC BY-SA 4.0 (staka/fugumt-ja-en). That travels with the
    weights — see project notes §8.

.PARAMETER Destination
    Where to put the model. Defaults to the app data directory Tauri uses.

.PARAMETER Force
    Re-download even if a file is already there.

.EXAMPLE
    pwsh -File scripts/fetch-translation-model.ps1
#>
[CmdletBinding()]
param(
    [string] $Destination = (Join-Path $env:APPDATA 'com.sakatsu.app\models\fugumt-ja-en'),
    [switch] $Force
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repository = 'Kadonox/fugumt-ja-en-onnx'
# A pinned revision, as `core/src/setup/catalog.rs` pins it: `main` can move under a reader.
$revision = '9b09cd7e6263223ac4a5988da1d881ef02b55be8'
$base = "https://huggingface.co/$repository/resolve/$revision"

$files = @(
    @{ Name = 'encoder_model.onnx'; Remote = 'onnx/encoder_model.onnx'; About = 'the encoder (~142 MB)' }
    @{ Name = 'decoder_model.onnx'; Remote = 'onnx/decoder_model.onnx'; About = 'the decoder (~168 MB)' }
    # The fast-tokenizer format, which avoids needing SentencePiece at all.
    @{ Name = 'tokenizer.json'; Remote = 'tokenizer.json'; About = 'the tokenizer (~3 MB)' }
    @{ Name = 'generation_config.json'; Remote = 'generation_config.json'; About = 'token ids' }
    @{ Name = 'config.json'; Remote = 'config.json'; About = 'model configuration' }
)

if (-not (Test-Path $Destination)) {
    New-Item -ItemType Directory -Force -Path $Destination | Out-Null
}
Write-Output "Source     : https://huggingface.co/$repository"
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
        Invoke-WebRequest -Uri "$base/$($file.Remote)" -OutFile $partial -MaximumRedirection 5
        Move-Item -Path $partial -Destination $path -Force
        $size = [math]::Round((Get-Item $path).Length / 1MB, 1)
        Write-Output "$($file.Name): done (${size} MB)"
    }
    catch {
        if (Test-Path $partial) { Remove-Item $partial -Force }
        Write-Output "$($file.Name): FAILED — $($_.Exception.Message)"
        continue
    }
}

$total = [math]::Round((Get-ChildItem $Destination -File | Measure-Object -Property Length -Sum).Sum / 1MB, 1)
Write-Output "`nTotal: ${total} MB in $Destination"
Write-Output "Licence: FuguMT is CC BY-SA 4.0 (staka/fugumt-ja-en)."
