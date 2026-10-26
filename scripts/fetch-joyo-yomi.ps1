<#
.SYNOPSIS
    Fetch the JoyoKanji-Yomi benchmark — an independent test of the readings this app shows.

.DESCRIPTION
    ## What it is for

    The reading tests that exist are this project's own: the 220-case heteronym fixture was built
    here, from spellings chosen here. This benchmark was built by SB Intuitions for text-to-speech:
    every reading of all 2,136 Joyo kanji, three native-speaker-verified sentences each, 13,095 in
    all. `core/tests/joyo_readings.rs` scores the furigana the reader shows against it.

    ## What it cannot do

    Its gold is a **pronunciation**, in phonetic katakana — long vowels as ー, は as ワ — so it
    can say whether the app's reading sounds right, never whether the furigana is *spelled* right.
    せい and せー are one answer to it. The scoring keeps those two questions apart.

    ## Where it goes

    **Not into the app's profile.** Evaluation data lives under `$env:SAKATSU_EVAL_DIR` (or
    `target\sakatsu-eval` in the repository when that is unset), beside the other isolated copies,
    and the harness finds it the same way (`core/tests/common/joyo.rs`); `SAKATSU_JOYO` overrides.

    ## Pinned

    One revision, and the file is checked against its recorded SHA-256 — a benchmark that can
    change underneath a recorded result is not a benchmark.

    ## Licence

    MIT. The licence file is fetched beside the data. Fetched, never committed.

.PARAMETER Out
    Where to write.
#>
[CmdletBinding()]
param(
    [string]$Out = $(if ($env:SAKATSU_EVAL_DIR) { Join-Path $env:SAKATSU_EVAL_DIR 'joyo-yomi' }
                     else { Join-Path $PSScriptRoot '..\target\sakatsu-eval\joyo-yomi' })
)

$ErrorActionPreference = 'Stop'

$REVISION = '2ae383bcdac88dacf89c9ab84f9990eec55fe5c9'
$BASE = "https://huggingface.co/datasets/sbintuitions/joyo-kanji-yomi-benchmark/resolve/$REVISION"
$FILES = @(
    @{ Name = 'common_kanji_source.jsonl'; Sha256 = '32a843f662c55f1876baf64f8dc3475cc7b777c576a8fea6b43a2343ff46e897'; Bytes = 2719205 },
    @{ Name = 'LICENSE'; Sha256 = 'f400180e4b1fdf66e97e8e38afbf19e0902ae6aa84decfb557e5c4019423e9d7'; Bytes = 1069 }
)

New-Item -ItemType Directory -Force -Path $Out | Out-Null

foreach ($file in $FILES) {
    $destination = Join-Path $Out $file.Name
    if (Test-Path $destination) {
        $have = (Get-FileHash -Algorithm SHA256 $destination).Hash.ToLowerInvariant()
        if ($have -eq $file.Sha256) {
            Write-Host "  $($file.Name) already here, verified" -ForegroundColor DarkGray
            continue
        }
        throw "$destination exists but does not match the pinned revision ($have). Move it aside; it is not overwritten."
    }

    Write-Host "  fetching $($file.Name)" -ForegroundColor Cyan
    $partial = "$destination.partial"
    Invoke-WebRequest -Uri "$BASE/$($file.Name)" -OutFile $partial -UseBasicParsing

    $length = (Get-Item $partial).Length
    $hash = (Get-FileHash -Algorithm SHA256 $partial).Hash.ToLowerInvariant()
    if ($length -ne $file.Bytes -or $hash -ne $file.Sha256) {
        Remove-Item $partial -Force
        throw "$($file.Name): expected $($file.Bytes) bytes and $($file.Sha256), got $length and $hash; removed."
    }
    Move-Item $partial $destination
}

# The attribution travels with the data, so it is written beside it rather than left to a reader of
# this script.
@"
JoyoKanji-Yomi Benchmark
https://huggingface.co/datasets/sbintuitions/joyo-kanji-yomi-benchmark
Revision $REVISION

Copyright (c) 2026 SB Intuitions. MIT License (LICENSE, beside this file).

Fetched by scripts/fetch-joyo-yomi.ps1 for evaluation only. Not committed, not shipped.
"@ | Set-Content -Path (Join-Path $Out 'ATTRIBUTION.txt') -Encoding utf8

Write-Host "`n  $Out" -ForegroundColor Green
Write-Host "  read it with SAKATSU_JOYO=$Out"
