<#
.SYNOPSIS
    Downloads the JPDB frequency list, which orders search results by how common a word is.

.DESCRIPTION
    An English search answers with several words that mean the same thing, and the
    dictionary cannot rank them: 22,553 of Jitendex's entries share its maximum score,
    食べる and 喫する and 食う among them, and the format carries no frequency data at all.
    A frequency list is where an order comes from.

    ## Which Japanese it counted

    JPDB's corpus is anime, visual novels and novels. That matters, because the three lists
    this app offers disagree — BCCWJ counts newspapers and white papers, Innocent Corpus
    counts light novels — and none of them is wrong. They counted different Japanese. This
    is the closest of them to what 屠つ is usually pointed at, which is why it is here, and
    the app names whichever list produced an order rather than presenting it as how common
    a word simply *is*.

    ## Ranks, not counts

    The numbers are ranks: の is 1, 食べる is 184, 顰蹙 is 68,420. Smaller is commoner.
    That is checked rather than assumed, and it matters: read as occurrence counts the
    order comes out exactly backwards, with 食べる last and every part of the machinery
    working perfectly. `Direction::Rank` in `core/src/dict/frequency.rs` is the setting
    that says so.

    ## Licence

    Compiled from https://jpdb.io/ and redistributed by the Yomitan dictionary collection
    linked below. Fetched, never committed (project notes §8), and the app shows the
    attribution beside it.

    https://github.com/Kuuuube/yomitan-dictionaries

.PARAMETER Out
    Where to write. Defaults to where the app reads frequency lists from.

.EXAMPLE
    pwsh -File scripts/fetch-frequency-jpdb.ps1
#>
[CmdletBinding()]
param(
    [string]$Out = (Join-Path $env:APPDATA 'com.sakatsu.app\dictionaries\frequency\jpdb'),
    [switch]$Force
)

$ErrorActionPreference = 'Stop'

$SOURCE = 'https://github.com/Kuuuube/yomitan-dictionaries/raw/main/dictionaries/JPDB_v2.2_Frequency_Kana_2024-10-13.zip'

New-Item -ItemType Directory -Force -Path $Out | Out-Null
$work = Join-Path $Out 'work'
New-Item -ItemType Directory -Force -Path $work | Out-Null
$zip = Join-Path $work 'jpdb-frequency.zip'

Write-Host "JPDB v2.2 frequency (from jpdb.io)" -ForegroundColor Cyan
if ($Force -or -not (Test-Path $zip)) {
    Invoke-WebRequest -Uri $SOURCE -OutFile $zip -UseBasicParsing
}

# Only the two kinds of file the ranking reads. A frequency zip can also carry term banks
# inherited from whatever it was built from, and those are a word dictionary's job — they
# would put a second copy of the definitions in front of the reader under a name that
# means "how common".
Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive = [System.IO.Compression.ZipFile]::OpenRead($zip)
try {
    $taken = 0
    foreach ($entry in $archive.Entries) {
        $name = $entry.Name
        if ($name -eq 'index.json' -or ($name -like 'term_meta_bank*' -and $name -like '*.json')) {
            $to = Join-Path $Out $name
            [System.IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $to, $true)
            $taken++
        }
    }
    Write-Host "  $taken files into $Out"
} finally {
    $archive.Dispose()
}

Write-Host "  Set 'Order results by' to JPDB in Settings, then restart." -ForegroundColor DarkGray
