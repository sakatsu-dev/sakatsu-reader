<#
.SYNOPSIS
    Build the noun → semantic-category table that sharpens particle senses.

.DESCRIPTION
    ## What it is for

    で marks where an action happens, what it is done with, and what a thing is made of,
    and the thing that separates those is what kind of noun sits in front: 図書館 is a
    place, 電車 is a made thing, 木 is a material. The app can rank で's senses without
    this — see `core/src/grammar/particles.rs` — but only on structural cues, so it
    abstains often. This is what lets it say "図書館 is a place" instead of shrugging.

    ## Two sources, joined here rather than at runtime

      Japanese WordNet   wnjpn-ok.tab   Japanese word → Princeton synset id
      Princeton WordNet  data.noun      synset → lexicographer file

    The lexicographer file is already the category wanted — noun.location, noun.artifact,
    noun.substance, noun.time, noun.person, noun.event. No hypernym walk is needed, which
    matters: the synset *hierarchy* ships only in WordNet-ja's SQLite build, and project notes
    §6 ruled out a native database for dictionary data. The lexname sidesteps it.

    Doing the join here means the app loads one flat `word<TAB>category` file and needs no
    parser for either source format.

    ## Licences

    Japanese WordNet — BSD-like. Requires attribution to NICT and Francis Bond and a link
    to https://bond-lab.github.io/wnja/ from anything that uses it.

    Princeton WordNet — WordNet License, BSD-like, attribution required.

    Both are fetched, never committed, and are listed in project notes §8 beside Jitendex.

.PARAMETER Out
    Where to write. Defaults beside the dictionaries.
#>
[CmdletBinding()]
param(
    [string]$Out = (Join-Path $env:APPDATA 'com.sakatsu.app\wordnet')
)

$ErrorActionPreference = 'Stop'

$WNJA = 'https://github.com/bond-lab/wnja/releases/download/v1.1/wnjpn-ok.tab.gz'
$PWN = 'https://wordnetcode.princeton.edu/3.0/WNdb-3.0.tar.gz'

# The classes a particle sense can turn on. Everything else collapses away — the table is
# for disambiguating particles, not for representing WordNet.
$KEEP = @(
    'noun.location', 'noun.artifact', 'noun.substance', 'noun.time',
    'noun.quantity', 'noun.event', 'noun.state', 'noun.phenomenon',
    'noun.person', 'noun.group', 'noun.food', 'noun.animal', 'noun.plant', 'noun.body'
)

New-Item -ItemType Directory -Force -Path $Out | Out-Null
$work = Join-Path $Out 'work'
New-Item -ItemType Directory -Force -Path $work | Out-Null

function Expand-Gzip($from, $to) {
    $in = [System.IO.File]::OpenRead($from)
    $out = [System.IO.File]::Create($to)
    $gz = New-Object System.IO.Compression.GzipStream($in, [System.IO.Compression.CompressionMode]::Decompress)
    $gz.CopyTo($out)
    $gz.Dispose(); $out.Dispose(); $in.Dispose()
}

Write-Host "Japanese WordNet (NICT / Francis Bond, BSD-like)" -ForegroundColor Cyan
$jaGz = Join-Path $work 'wnjpn-ok.tab.gz'
$jaTab = Join-Path $work 'wnjpn-ok.tab'
if (-not (Test-Path $jaTab)) {
    Invoke-WebRequest -Uri $WNJA -OutFile $jaGz -UseBasicParsing
    Expand-Gzip $jaGz $jaTab
}
Write-Host "  $([math]::Round((Get-Item $jaTab).Length / 1MB, 1)) MB"

Write-Host "Princeton WordNet 3.0 (WordNet License, BSD-like)" -ForegroundColor Cyan
$pwnTar = Join-Path $work 'WNdb-3.0.tar.gz'
$dataNoun = Join-Path $work 'dict\data.noun'
if (-not (Test-Path $dataNoun)) {
    Invoke-WebRequest -Uri $PWN -OutFile $pwnTar -UseBasicParsing
    tar -xzf $pwnTar -C $work
}
Write-Host "  $([math]::Round((Get-Item $dataNoun).Length / 1MB, 1)) MB"

# ---------------------------------------------------------------------------------------
# synset offset → lexicographer file.
#
# data.noun is a flat format: `offset lex_filenum ss_type ...`, one synset per line, with
# a licence header whose lines start with two spaces.
# ---------------------------------------------------------------------------------------
Write-Host "`nreading synset categories..."
$lexnames = @{}
foreach ($line in [System.IO.File]::ReadLines($dataNoun)) {
    if ($line.StartsWith('  ')) { continue }
    $parts = $line.Split(' ', 4)
    if ($parts.Count -lt 3) { continue }
    $lexnames[$parts[0]] = [int]$parts[1]
}
Write-Host "  $($lexnames.Count) synsets"

# lex_filenum → name, from WordNet's own lexnames table. Only the noun files are listed;
# the numbers are stable across releases.
$byNumber = @{
    3 = 'noun.Tops'; 4 = 'noun.act'; 5 = 'noun.animal'; 6 = 'noun.artifact'
    7 = 'noun.attribute'; 8 = 'noun.body'; 9 = 'noun.cognition'; 10 = 'noun.communication'
    11 = 'noun.event'; 12 = 'noun.feeling'; 13 = 'noun.food'; 14 = 'noun.group'
    15 = 'noun.location'; 16 = 'noun.motive'; 17 = 'noun.object'; 18 = 'noun.person'
    19 = 'noun.phenomenon'; 20 = 'noun.plant'; 21 = 'noun.possession'; 22 = 'noun.process'
    23 = 'noun.quantity'; 24 = 'noun.relation'; 25 = 'noun.shape'; 26 = 'noun.state'
    27 = 'noun.substance'; 28 = 'noun.time'
}

# ---------------------------------------------------------------------------------------
# Japanese word → category.
#
# wnjpn-ok.tab is `synset<TAB>word<TAB>source`, synsets as `06294441-n`. A word can belong
# to several synsets; the commonest category wins, and a tie is left out rather than
# guessed at — a wrong category produces a confidently wrong particle reading, which is
# worse than the abstention it replaces.
# ---------------------------------------------------------------------------------------
Write-Host "joining..."
# Ordinal, because `@{}` is case-*insensitive* in PowerShell and these keys are words.
# With the default comparer, Au (gold), AU (astronomical unit) and au all landed on one
# key: their votes merged, one winner was written under whichever spelling arrived first,
# and the rest vanished. H and h — hydrogen and hour — went the same way. Found by
# `core/tests/setup_build.rs` comparing this against the Rust builder, which is
# case-sensitive and kept them apart.
$votes = [System.Collections.Hashtable]::new([System.StringComparer]::Ordinal)
foreach ($line in [System.IO.File]::ReadLines($jaTab)) {
    $parts = $line.Split("`t")
    if ($parts.Count -lt 2) { continue }
    $synset = $parts[0]
    if (-not $synset.EndsWith('-n')) { continue }   # nouns only
    $offset = $synset.Substring(0, $synset.Length - 2)
    $num = $lexnames[$offset]
    if ($null -eq $num) { continue }
    $name = $byNumber[$num]
    if (-not $name -or $KEEP -notcontains $name) { continue }

    $word = $parts[1]
    if (-not $votes.ContainsKey($word)) { $votes[$word] = @{} }
    # Spelled out rather than with `??`, which is PowerShell 7 only and stops this script
    # parsing in the Windows PowerShell that ships with the operating system.
    $seen = $votes[$word][$name]
    if ($null -eq $seen) { $seen = 0 }
    $votes[$word][$name] = $seen + 1
}

$lines = [System.Collections.Generic.List[string]]::new()
$ambiguous = 0
foreach ($word in $votes.Keys) {
    $ranked = $votes[$word].GetEnumerator() | Sort-Object Value -Descending
    if ($ranked.Count -gt 1 -and $ranked[0].Value -eq $ranked[1].Value) {
        $ambiguous++
        continue
    }
    $lines.Add("$word`t$($ranked[0].Key)")
}
# Ordinal, not the current culture's order. `Sort()` with no comparer is
# culture-sensitive, so the same input produced a differently ordered file on a machine
# with a different locale — and `-冊` sorted before `$` here but need not elsewhere.
# Nothing reading this file cares about order, but a build artifact that varies by
# locale is not reproducible, and the Rust builder sorts ordinally.
$lines.Sort([System.StringComparer]::Ordinal)

$dest = Join-Path $Out 'categories.tsv'
[System.IO.File]::WriteAllLines($dest, $lines, [System.Text.UTF8Encoding]::new($false))

Write-Host ""
Write-Host "wrote $dest" -ForegroundColor Green
Write-Host "  $($lines.Count) words, $([math]::Round((Get-Item $dest).Length / 1MB, 2)) MB"
Write-Host "  $ambiguous left out — their categories tied, and a coin-toss category makes a confidently wrong reading"
Write-Host ""
Write-Host "Japanese WordNet: https://bond-lab.github.io/wnja/ — NICT, Francis Bond, BSD-like" -ForegroundColor DarkGray
Write-Host "Princeton WordNet 3.0: https://wordnet.princeton.edu/ — WordNet License" -ForegroundColor DarkGray
