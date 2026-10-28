<#
.SYNOPSIS
    Build the kanji details used by the reference page.

.DESCRIPTION
    ## What it is for

    The installed dictionaries answer questions about *words*. Kun'yomi and on'yomi are
    properties of *characters*, and neither Jitendex nor JMnedict ships a kanji bank — so
    without this the app cannot say what a character is or how it is read. Looking up 館
    returns the word やかた "mansion", which is not what 館 does in 図書館; 勉 and 聞
    return nothing at all.

    ## One flat table, parsed here rather than at runtime

    KANJIDIC2 is a 15 MB XML file. Parsing it here means the app loads a
    `char<TAB>on<TAB>kun<TAB>meanings<TAB>strokes<TAB>grade<TAB>nanori` file of about a megabyte and
    needs no XML parser at all — the same trade `scripts/fetch-wordnet.ps1` makes, and in
    the spirit of project notes §6's refusal of a native database for dictionary data.

    Read with XmlReader rather than the [xml] accelerator: the accelerator builds a DOM
    for the whole 15 MB to read six fields per character.

    ## What is kept, and what is not

    Readings are stored **verbatim**. A kun reading writes its okurigana after a period —
    た.べる is stem た plus べる — and that period is what lets the app work out which
    reading a word actually uses. Stripping it here would throw the information away.

    Kept separately:
      nanori   name-only readings. They are displayed as names but the reading matcher never
               sees them, so 明日 cannot incorrectly "align" as あす+ひ.

    This script also names and fetches the same pinned supplemental sources as the in-app setup.
    The Rust setup path performs their strict conversion and is the supported installation path;
    this script remains the reproducible KANJIDIC table builder and source-staging path used by
    the cross-check tests.
      jlpt     the pre-2010 four-level scale. The grammar packs all speak N5-N1, and two
               different scales wearing the same word is worse than one missing field.
      pinyin, korean, vietnamese, and non-English meanings.

    ## Licence

    KANJIDIC2 — Electronic Dictionary Research and Development Group (EDRDG),
    **CC BY-SA 4.0**. The same rights holder and licence as the JMdict-derived data the
    app already ships, so this takes on no new obligation beyond naming it — which the
    setup panel does. Fetched, never committed (project notes §8).

    https://www.edrdg.org/wiki/index.php/KANJIDIC_Project

.PARAMETER Out
    Where to write. Defaults beside the dictionaries.
#>
[CmdletBinding()]
param(
    [string]$Out = (Join-Path $env:APPDATA 'com.sakatsu.app\kanji')
)

$ErrorActionPreference = 'Stop'

# The publisher replaces its original URL; use the same immutable snapshot as setup.
$SOURCE = 'https://github.com/sakatsu-dev/reader-resources/releases/download/kanjidic2-2026-10-18/kanjidic2-20261018.xml.gz'
$KANJIDIC_BYTES = 1488573
$KANJIDIC_SHA256 = '6182a700e08a8f6a6646529b77f84f0d963ed33cd5c7516ff19e4ee642345446'
$KANJIVG_SOURCE = 'https://github.com/KanjiVG/kanjivg/releases/download/r20260714/kanjivg-20260714-main.zip'
$KRAD_SOURCE = 'http://ftp.edrdg.org/pub/Nihongo/kradfile.gz'

New-Item -ItemType Directory -Force -Path $Out | Out-Null
$work = Join-Path $Out 'work'
New-Item -ItemType Directory -Force -Path $work | Out-Null

$gz = Join-Path $work 'kanjidic2.xml.gz'
$xml = Join-Path $work 'kanjidic2.xml'
$kanjivg = Join-Path $work 'kanjivg-20260714-main.zip'
$krad = Join-Path $work 'kradfile.gz'

Write-Host "KANJIDIC2 (EDRDG, CC BY-SA 4.0)" -ForegroundColor Cyan
if (-not (Test-Path $xml)) {
    Invoke-WebRequest -Uri $SOURCE -OutFile $gz -UseBasicParsing
    if ((Get-Item -LiteralPath $gz).Length -ne $KANJIDIC_BYTES -or
        (Get-FileHash -LiteralPath $gz -Algorithm SHA256).Hash -ne $KANJIDIC_SHA256) {
        throw 'KANJIDIC2 does not match the pinned snapshot; no table was built.'
    }
    # PowerShell names are case-insensitive: $out would overwrite the typed Out parameter.
    # Publish XML only after decompression completes, so a retry never reuses a partial file.
    $xmlStage = "$xml.$([guid]::NewGuid().ToString('N')).partial"
    try {
        $archiveInput = [System.IO.File]::OpenRead($gz)
        try {
            $xmlOutput = [System.IO.File]::Create($xmlStage)
            try {
                $gzipReader = New-Object System.IO.Compression.GzipStream($archiveInput, [System.IO.Compression.CompressionMode]::Decompress)
                try { $gzipReader.CopyTo($xmlOutput) } finally { $gzipReader.Dispose() }
            } finally { $xmlOutput.Dispose() }
        } finally { $archiveInput.Dispose() }
        Move-Item -LiteralPath $xmlStage -Destination $xml
    } finally {
        if (Test-Path -LiteralPath $xmlStage) { Remove-Item -LiteralPath $xmlStage -Force }
    }
}
Write-Host "  $([math]::Round((Get-Item $xml).Length / 1MB, 1)) MB"

# Stage the two supplemental archives beside KANJIDIC. The desktop setup sanitizes and publishes
# them as typed diagrams and component tables; retaining these originals keeps the independent
# developer cross-check reproducible without committing fetched data.
if (-not (Test-Path $kanjivg)) { Invoke-WebRequest -Uri $KANJIVG_SOURCE -OutFile $kanjivg -UseBasicParsing }
if (-not (Test-Path $krad)) { Invoke-WebRequest -Uri $KRAD_SOURCE -OutFile $krad -UseBasicParsing }

# ---------------------------------------------------------------------------------------
# Stream the characters out.
#
# The DTD is inline and declares entities, so the reader is told not to resolve anything
# from the network and given a null resolver — a fetch script should not make a second,
# unannounced request because a document asked it to.
# ---------------------------------------------------------------------------------------
Write-Host "`nreading characters..."

$settings = New-Object System.Xml.XmlReaderSettings
$settings.DtdProcessing = [System.Xml.DtdProcessing]::Parse
$settings.XmlResolver = $null
$settings.IgnoreWhitespace = $true
$settings.IgnoreComments = $true

$reader = [System.Xml.XmlReader]::Create($xml, $settings)
$lines = [System.Collections.Generic.List[string]]::new()
$noReadings = 0

# `.` separates a kun reading's stem from its okurigana and must survive; `,` and `;` are
# the field separators, so a value containing one would silently split into two.
$illegal = @("`t", ',', ';')

try {
    while ($reader.ReadToFollowing('character')) {
        $literal = $null
        $on = [System.Collections.Generic.List[string]]::new()
        $kun = [System.Collections.Generic.List[string]]::new()
        $meanings = [System.Collections.Generic.List[string]]::new()
        $nanori = [System.Collections.Generic.List[string]]::new()
        $strokes = ''
        $grade = ''
        # Stroke counts are listed most-common-first, with later ones being miscounts the
        # file records for search purposes. Only the first is the answer.
        $haveStrokes = $false

        $entry = $reader.ReadSubtree()
        $entry.Read() | Out-Null

        # ReadElementContentAsString consumes the element and leaves the reader on the
        # node *after* its end tag. So the loop advances only when nothing was read —
        # advancing unconditionally silently skips every second sibling, which cost 食
        # its ジキ and every character its stroke count before it was caught by reading
        # the output back against the XML.
        while (-not $entry.EOF) {
            if ($entry.NodeType -ne [System.Xml.XmlNodeType]::Element) {
                if (-not $entry.Read()) { break }
                continue
            }
            # if/elseif rather than switch: in PowerShell `switch` is itself a loop
            # construct, and `break`/`continue` inside one do not mean what they read as.
            $name = $entry.Name
            if ($name -eq 'literal') {
                $literal = $entry.ReadElementContentAsString()
            }
            elseif ($name -eq 'stroke_count') {
                $value = $entry.ReadElementContentAsString()
                if (-not $haveStrokes) { $strokes = $value; $haveStrokes = $true }
            }
            elseif ($name -eq 'grade') {
                $grade = $entry.ReadElementContentAsString()
            }
            elseif ($name -eq 'reading') {
                $type = $entry.GetAttribute('r_type')
                $value = $entry.ReadElementContentAsString()
                if ($type -eq 'ja_on') { $on.Add($value) }
                elseif ($type -eq 'ja_kun') { $kun.Add($value) }
            }
            elseif ($name -eq 'meaning') {
                # English is the unmarked default; every other language names itself.
                $lang = $entry.GetAttribute('m_lang')
                $value = $entry.ReadElementContentAsString()
                if ([string]::IsNullOrEmpty($lang)) { $meanings.Add($value) }
            }
            elseif ($name -eq 'nanori') {
                $nanori.Add($entry.ReadElementContentAsString())
            }
            else {
                if (-not $entry.Read()) { break }
            }
        }
        $entry.Dispose()

        if (-not $literal) { continue }
        if ($on.Count -eq 0 -and $kun.Count -eq 0 -and $meanings.Count -eq 0) {
            # A character with nothing to say about it — JIS X 0212 fillers, mostly.
            $noReadings++
            continue
        }

        $clean = {
            param($values, $joiner)
            $kept = foreach ($v in $values) {
                $bad = $false
                foreach ($c in $illegal) { if ($v.Contains($c)) { $bad = $true } }
                if (-not $bad) { $v }
            }
            ($kept -join $joiner)
        }

        $lines.Add(("{0}`t{1}`t{2}`t{3}`t{4}`t{5}`t{6}" -f
            $literal,
            (& $clean $on ','),
            (& $clean $kun ','),
            (& $clean $meanings ';'),
            $strokes,
            $grade,
            (& $clean $nanori ',')))
    }
}
finally {
    $reader.Dispose()
}

# Ordinal, not the current culture's order. `Sort()` with no comparer is
# culture-sensitive, so the same input produced a differently ordered file on a machine
# with a different locale — and `-冊` sorted before `$` here but need not elsewhere.
# Nothing reading this file cares about order, but a build artifact that varies by
# locale is not reproducible, and the Rust builder sorts ordinally.
$lines.Sort([System.StringComparer]::Ordinal)
$dest = Join-Path $Out 'kanjidic.tsv'
[System.IO.File]::WriteAllLines($dest, $lines, [System.Text.UTF8Encoding]::new($false))

Write-Host ""
Write-Host "wrote $dest" -ForegroundColor Green
Write-Host "  $($lines.Count) characters, $([math]::Round((Get-Item $dest).Length / 1MB, 2)) MB"
if ($noReadings -gt 0) {
    Write-Host "  $noReadings skipped — no readings and no meanings, so nothing to show"
}
Write-Host ""
Write-Host "KANJIDIC2: https://www.edrdg.org/wiki/index.php/KANJIDIC_Project" -ForegroundColor DarkGray
Write-Host "  Electronic Dictionary Research and Development Group, CC BY-SA 4.0" -ForegroundColor DarkGray
Write-Host "KanjiVG r20260714: https://github.com/KanjiVG/kanjivg/releases/tag/r20260714" -ForegroundColor DarkGray
Write-Host "  Ulrich Apel and contributors, CC BY-SA 3.0" -ForegroundColor DarkGray
Write-Host "basic KRADFILE: https://www.edrdg.org/krad/kradinf.html" -ForegroundColor DarkGray
Write-Host "  Electronic Dictionary Research and Development Group licence" -ForegroundColor DarkGray
