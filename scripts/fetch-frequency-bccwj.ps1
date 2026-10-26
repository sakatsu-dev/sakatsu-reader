<#
.SYNOPSIS
    Downloads the BCCWJ frequency list, which orders search results by how common a word is.

.DESCRIPTION
    An English search answers with several words that mean the same thing, and the
    dictionary cannot rank them: 22,553 of Jitendex's entries share its maximum score,
    食べる and 喫する and 食う among them, and the format carries no frequency data at all.
    A frequency list is where an order comes from.

    ## Which Japanese it counted

    NINJAL's Balanced Corpus of Contemporary Written Japanese: a hundred million words of
    books, magazines, newspapers, white papers, textbooks and blogs. The widest coverage of
    the three this app offers, and the citable one — but balanced across written Japanese
    as a whole, which pulls it away from fiction. JPDB counts anime and visual novels and
    disagrees with it, and neither is wrong: they counted different Japanese. The app names
    whichever list produced an order rather than presenting it as how common a word simply
    *is*.

    ## Ranks, not counts

    The numbers are ranks: の is 1, 食べる is 199, 顰蹙 is 28,602. Smaller is commoner.
    That is checked rather than assumed, and it matters: read as occurrence counts the
    order comes out exactly backwards, with 食べる last and every part of the machinery
    working perfectly. `Direction::Rank` in `core/src/dict/frequency.rs` is the setting
    that says so.

    ## Licence

    Derived from NINJAL's BCCWJ and redistributed by the Yomitan dictionary collection
    linked below, which builds on toasted-nutbread's frequency dictionary. Fetched, never
    committed (project notes §8), and the app shows the attribution beside it.

    https://github.com/Kuuuube/yomitan-dictionaries

.PARAMETER Out
    Where to write. Defaults to where the app reads frequency lists from.

.EXAMPLE
    pwsh -File scripts/fetch-frequency-bccwj.ps1
#>
[CmdletBinding()]
param(
    [string]$Out = (Join-Path $env:APPDATA 'com.sakatsu.app\dictionaries\frequency\bccwj'),
    [switch]$Force
)

$ErrorActionPreference = 'Stop'

$SOURCE = 'https://github.com/Kuuuube/yomitan-dictionaries/raw/main/dictionaries/BCCWJ_SUW_LUW_combined.zip'

New-Item -ItemType Directory -Force -Path $Out | Out-Null
$work = Join-Path $Out 'work'
New-Item -ItemType Directory -Force -Path $work | Out-Null
$zip = Join-Path $work 'bccwj-frequency.zip'

Write-Host "BCCWJ frequency (NINJAL)" -ForegroundColor Cyan
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

Write-Host "  Set 'Order results by' to BCCWJ in Settings, then restart." -ForegroundColor DarkGray
