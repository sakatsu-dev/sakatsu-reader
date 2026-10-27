<#
.SYNOPSIS
    Downloads the PP-OCRv6 (small) recognition models into the user profile.

.DESCRIPTION
    屠つ ships by fetch, not by bundle (project notes §6 applies to models as much as to
    dictionaries): this is ~30 MB and is never committed to this repository.

    Nothing here runs automatically. It is a deliberate, user-initiated download.

    PP-OCR is the *second* recognition engine, alongside manga-ocr, and the two are
    complementary rather than ranked. manga-ocr is trained on manga and is the one to use for
    vertical text, furigana and hand lettering; PP-OCR is trained on everyday printed text and
    reads the typeset text of a game or a menu better. Auto routes between them.

    PP-OCRv6 small replaced PP-OCRv5 on 2026-10-09: on the capture corpus it read with less than
    half v5's character error in a third of the time (local documentation).

    Unlike manga-ocr, this is a pipeline: a detector finds the text lines and a recogniser reads
    each one, so there are two models plus the character dictionary they are decoded against.
    PaddlePaddle publishes v6 in Paddle format only; these are snowfluke's ONNX conversion of the
    official weights, pinned to one revision and checked by SHA-256. The files are renamed on
    arrival to the names the loader reads.

    Licence: Apache-2.0 — PaddlePaddle; the ONNX conversion is also Apache-2.0. See project notes §8.

.PARAMETER Destination
    Where to put the models. Defaults to the app data directory Tauri uses for
    identifier `com.sakatsu.app` on Windows.

.PARAMETER Force
    Re-download even if a file is already there.

.EXAMPLE
    pwsh -File scripts/fetch-paddle-ocr.ps1
#>
[CmdletBinding()]
param(
    [string] $Destination = (Join-Path $env:APPDATA 'com.sakatsu.app\models\ppocrv6'),
    [switch] $Force
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repository = 'snowfluke/ppu-paddle-ocr-models'
$revision = 'bf1d5edb0335d3262be7caf13f766ba274b4cadd'
$base = "https://huggingface.co/$repository/resolve/$revision"

# Size and SHA-256 per file, from the repository's own records at that revision. A file that
# does not match both is refused: a truncated or substituted model loads and then reads badly,
# which is far harder to diagnose than a failed download.
$files = @(
    @{ Source = 'detection/PP-OCRv6_small_det.onnx'; Name = 'det.onnx'; Bytes = 9880512
       Sha256 = 'd73e0058b7a8086bbd57f3d10b8bcd4ff95363f67e06e2762b5e814fe9c9410e' }
    @{ Source = 'recognition/PP-OCRv6_small_rec.onnx'; Name = 'rec.onnx'; Bytes = 21159378
       Sha256 = '5435fd747c9e0efe15a96d0b378d5bd157e9492ed8fd80edf08f30d02fa24634' }
    # Small and load-bearing: the recogniser emits indices, and this is what turns them into
    # Japanese. 屠つ checks its length against the model's class count on load and refuses the
    # pair if they disagree, because a mismatched dictionary decodes into fluent wrong text.
    @{ Source = 'recognition/ppocrv6_dict.txt'; Name = 'ppocrv6_dict.txt'; Bytes = 74948
       Sha256 = '41557512862dfe31970cf22407742b629725461dd84c0d8771bde9c87c2202c8' }
)

if (-not (Test-Path $Destination)) {
    New-Item -ItemType Directory -Force -Path $Destination | Out-Null
}
Write-Output "Destination: $Destination`n"

$failed = 0
foreach ($file in $files) {
    $path = Join-Path $Destination $file.Name
    if ((Test-Path $path) -and -not $Force) {
        Write-Output "$($file.Name): already present — pass -Force to replace it"
        continue
    }
    Write-Output "$($file.Name): downloading $($file.Source)"
    # Download beside the target and move into place, so an interrupted run cannot leave a
    # truncated model that then fails to load for no obvious reason.
    $partial = "$path.partial"
    try {
        Invoke-WebRequest -Uri "$base/$($file.Source)" -OutFile $partial -MaximumRedirection 5
        $length = (Get-Item $partial).Length
        if ($length -ne $file.Bytes) { throw "size $length, expected $($file.Bytes)" }
        $hash = (Get-FileHash -Algorithm SHA256 $partial).Hash.ToLowerInvariant()
        if ($hash -ne $file.Sha256) { throw "SHA-256 $hash, expected $($file.Sha256)" }
        Move-Item -Path $partial -Destination $path -Force
        Write-Output "$($file.Name): done"
    }
    catch {
        if (Test-Path $partial) { Remove-Item $partial -Force }
        Write-Output "$($file.Name): FAILED — $($_.Exception.Message)"
        $failed++
    }
}

Write-Output "`nLicence — this travels with the data and must be attributed:"
Write-Output "  PP-OCRv6: Apache-2.0 — PaddlePaddle; ONNX conversion by snowfluke, Apache-2.0"
Write-Output "`nChoose the engine in 屠つ's settings; manga-ocr stays the better choice for manga."
if ($failed) { exit 1 }
