<#
.SYNOPSIS
    Fetch JESC — subtitle Japanese, the closest freely-licensed register to this app's domain.

.DESCRIPTION
    ## What it is for

    `analyzer_corpus.rs` profiles where the two analyzers disagree, but only over what it is
    given, and what it has been given is wrong for the question now being asked. The corpus
    transcriptions are the right register and there are 141 of them. The 26,006 Tatoeba-derived
    sentences are volume, and they are short, clean, and frequently translated *from* English —
    the register the built-in analyzer already handles at 96%.

    The gap is casual speech, and JESC is 2.8M sentence pairs crawled from movie and television
    subtitles. Its own paper names its domain "casual language, colloquialisms" and calls that
    domain poorly represented, which is the same complaint this project has.

    Only the Japanese column is kept. The English is not needed — nothing here scores a
    translation — and dropping it halves what has to be written to disk.

    ## What it is NOT

    **It is not gold.** Nobody annotated boundaries or readings, so this can measure agreement
    and nothing else — the same limit `analyzer_corpus.rs` states in its own header, for the same
    reason. Use `ud_gold.rs` and `kwdlc_gold.rs` when the question is who is *right*.

    **The Japanese is of uneven quality.** JESC is crawled and automatically aligned, and the
    alignment is visibly imperfect: `no child should ever have to endure that.` is paired with
    `必要のない子 耐えるにしている。`, which is not grammatical Japanese. Misalignment itself is
    harmless here because the English is discarded, but broken Japanese is not — it produces
    disagreements that are the corpus's fault rather than either analyzer's. Read the sample
    before believing any of it.

    **It is standard casual, not dialect.** Subtitles for foreign film are rendered in standard
    Japanese; a translator choosing 〜だべ or 〜けんども is the exception. So this reaches 〜ちゃう,
    〜とく, 〜なきゃ and 〜じゃん in volume and does not reach the regional forms in the
    hand-collected corpus. The two sources are complementary and neither substitutes for the other.

    **Lines carry internal spaces.** A subtitle's on-screen line break survives as a space:
    `あなたは戻ったのね ハロルド?`. It is left in place deliberately. Both analyzers treat it as a
    boundary, so it suppresses compounds across the gap rather than inventing disagreements —
    stripping it would splice two utterances into a sentence nobody wrote.

    ## Two output files, because the Ichiran budget decides what is worth reading

    Ichiran costs **1.9 s a call on this corpus** and the Lisp image reloads per invocation, so
    2.8M lines is roughly 62 days and even 2,000 is an hour. *Which* 2,000 therefore matters far
    more than the count does.

    That 1.9 s is measured here and is double the 0.9 s `analyzer_corpus.rs` records against
    Tatoeba — subtitle lines are longer and less regular, and the sidecar takes proportionally
    longer on them. Budget from this number, not that one.

      jesc-sentences.txt  every line that survives filtering, deduplicated. Stride-sample this
                          for an *unbiased* disagreement profile of subtitle Japanese.

      jesc-casual.txt     the subset carrying a casual marker — see $MARKERS below. This is
                          targeted mining, not measurement: a disagreement rate computed on it
                          is NOT comparable to one from the full file, because the subset was
                          selected for the very property under investigation. It is the right
                          input for "which compounds does the built-in analyzer split wrongly",
                          and the wrong input for "how far apart are the two analyzers".

    ## Licence

    CC BY-SA 4.0 — https://nlp.stanford.edu/projects/jesc/. The same licence already carried for
    Jitendex, JMnedict, KANJIDIC2 and UD_Japanese-GSD, so no new obligation in kind is taken on.
    Attribution belongs in `project notes` §8 beside them. Fetched, never committed.

.PARAMETER Out
    Where the sentence files land. Defaults beside the other fetched corpora.

.PARAMETER Staging
    Where the 97 MB archive is unpacked. The extracted TSV is about 500 MB and is deleted once
    the column has been split, but it has to exist somewhere first — point this at a roomy drive
    if the default is tight.

.PARAMETER MinChars
    Shortest line to keep. Below about four characters a line says nothing about segmentation.

.PARAMETER MaxChars
    Longest line to keep. Runaway lines are concatenation failures in the crawl, not sentences.
#>
[CmdletBinding()]
param(
    [string]$Out = (Join-Path $env:APPDATA 'com.sakatsu.app\corpora\jesc'),
    [string]$Staging = (Join-Path $env:TEMP 'jesc-staging'),
    [int]$MinChars = 4,
    [int]$MaxChars = 120
)

$ErrorActionPreference = 'Stop'

$ARCHIVE = 'https://nlp.stanford.edu/projects/jesc/data/raw.tar.gz'

# The markers that select `jesc-casual.txt`. Contracted auxiliaries and casual sentence-enders —
# what `is_contracted_auxiliary` and `is_connective` in `analyzer/lindera/mod.rs` handle, and so
# where their failures will be. Deliberately NOT a dialect list: subtitles are standard casual,
# and asking for 〜だべ here returns almost nothing.
#
# **Named $MARKERS and not $CASUAL, which is not a style preference.** PowerShell variable names
# are case-insensitive, `$casual` below holds the output path, and the two were the same variable.
# The array was silently overwritten with a filename, `foreach` iterated once over that string,
# and the run reported 0 casual lines out of 2.4M without erroring.
# **Hex, and every comment ends in romaji, and neither is decoration.**
#
# Windows PowerShell 5.1 reads a BOM-less script in the machine's ANSI codepage — CP932 on a
# Japanese install — and a UTF-8 kana's third byte is often a CP932 *lead* byte. In code that
# swallows the quote after it; at the end of a comment it swallows the **newline**, and the
# comment eats the line below. る is E3 82 8B and 0x8B is a lead byte, so `# でる` at end of line
# stopped this whole file parsing. Measured, not deduced: `# でる` fails, `# でる.` parses,
# `# deru` parses.
#
# So the characters live in hex and are assembled below, and each comment ends on an ASCII
# syllable. The romaji is load-bearing — do not tidy it away — and it is also the reading, which
# is why it is romaji rather than a bare full stop. `src-tauri/tests/launched_scripts.rs` fails
# immediately if either rule is broken, so this cannot rot quietly. A byte-order mark would fix
# it too and is deliberately not the convention here; that file says why.
$MARKERS = @(
    '3061 3083 3063'       # ちゃっ chatt
    '3061 3083 3046'       # ちゃう chau
    '3061 3083 304a'       # ちゃお chao
    '3058 3083 3063'       # じゃっ jatt
    '3058 3083 3046'       # じゃう jau
    '3061 307e 3046'       # ちまう chimau
    '3058 307e 3046'       # じまう jimau
    '3068 308b'            # とる toru
    '3069 308b'            # どる doru
    '3068 304f'            # とく toku
    '3069 304f'            # どく doku
    '3066 308b'            # てる teru
    '3067 308b'            # でる deru
    '3066 305f'            # てた teta
    '3067 305f'            # でた deta
    '306a 304d 3083'       # なきゃ nakya
    '306a 304f 3061 3083'  # なくちゃ nakucha
    '306a 3051 308a 3083'  # なけりゃ nakerya
    '3058 3083 3093'       # じゃん jan
    '3058 3083 306a 3044'  # じゃない janai
    '3058 3083 306d 30fc'  # じゃねー janee
    '3058 3083 306d 3047'  # じゃねぇ janee (small e)
    '306d 30fc 3088'       # ねーよ neeyo
    '306d 3047 3088'       # ねぇよ neeyo (small e)
    '3060 308d'            # だろ daro
    '3067 3057 3087'       # でしょ desho
    '3063 3059'            # っす ssu
    '3093 3060'            # んだ nda
    '3093 3067'            # んで nde
    '304b 3088'            # かよ kayo
    '308f 3088'            # わよ wayo
    '304b 3057 3089'       # かしら kashira
    '3082 3093'            # もん mon
    '3063 3051'            # っけ kke
) | ForEach-Object {
    -join ($_.Split(' ') | ForEach-Object { [char][Convert]::ToInt32($_, 16) })
}

# **Checked where the list is used, not where it is written, because that is where it failed.**
# The array was once clobbered by a same-named path variable — PowerShell variable names are
# case-insensitive, and `$casual` below holds the output path — and no check placed next to the
# definition could have seen it, because the clobber happens further down.
#
# That failure raises no error on its own. It ends as an empty casual file, which reads as a
# corpus with no casual speech in it: the one result that sends you looking in the wrong place.
#
# The code-point check is cheap and stays: it costs nothing and would catch the assembly above
# being broken by an edit.
function Assert-Markers {
    if ($MARKERS -isnot [array] -or $MARKERS.Count -lt 30) {
        throw ('$MARKERS is not the marker list any more (' + $MARKERS.GetType().Name + ', ' +
            @($MARKERS).Count + ' items). Something reassigned it — note that PowerShell ' +
            'variable names are case-insensitive.')
    }
    if ([int]$MARKERS[0][0] -ne 0x3061) {
        throw ('The marker list did not assemble (' + $MARKERS[0] + '). The hex table above is ' +
            'split on spaces and converted per entry; an edit has broken that.')
    }
}

# ── Space, checked before anything is downloaded ─────────────────────────────────────────────
#
# A download that dies at 90% leaves a partial archive and a confusing error. The extracted TSV
# is the real cost: 97 MB compressed, about 500 MB open.
function Assert-Space {
    param([string]$Path, [double]$NeedGB, [string]$Flag)

    $probe = $Path
    while ($probe -and -not (Test-Path -LiteralPath $probe)) { $probe = Split-Path -Parent $probe }
    if (-not $probe) { return }

    $root = [System.IO.Path]::GetPathRoot((Resolve-Path -LiteralPath $probe).Path)
    $drive = Get-PSDrive -PSProvider FileSystem | Where-Object { $_.Root -eq $root }
    if (-not $drive) { return }

    $freeGB = [math]::Round($drive.Free / 1GB, 1)
    if ($drive.Free -lt ($NeedGB * 1GB)) {
        throw "$root has $freeGB GB free and this needs about $NeedGB GB. Pass -$Flag pointing at a roomier drive."
    }
    Write-Host "  $root has $freeGB GB free" -ForegroundColor DarkGray
}

New-Item -ItemType Directory -Force -Path $Out | Out-Null

$sentences = Join-Path $Out 'jesc-sentences.txt'
$casual = Join-Path $Out 'jesc-casual.txt'

# **Non-empty, not merely present.** A zero-byte output is what both of this script's failures
# left behind, and treating that as "already here" would make the fix look like it had run.
$done = @($sentences, $casual) | ForEach-Object { (Test-Path $_) -and (Get-Item $_).Length -gt 0 }
if ($done -notcontains $false) {
    $mb = [math]::Round((Get-Item $sentences).Length / 1MB, 1)
    Write-Host "  already here: $mb MB of sentences in $Out" -ForegroundColor DarkGray
    return
}

Assert-Space -Path $Staging -NeedGB 1.0 -Flag 'Staging'

New-Item -ItemType Directory -Force -Path $Staging | Out-Null
$zip = Join-Path $Staging 'raw.tar.gz'

if (-not (Test-Path $zip)) {
    Write-Host "  fetching JESC (97 MB)" -ForegroundColor Cyan
    Invoke-WebRequest -Uri $ARCHIVE -OutFile $zip -UseBasicParsing
}

# A truncated archive is worse than a missing one: tar reports a vague failure well into the
# extraction, after the disk has already been spent.
$size = (Get-Item $zip).Length
if ($size -lt 100MB) {
    Remove-Item $zip -Force
    throw "raw.tar.gz came back $([math]::Round($size / 1MB, 1)) MB, expected about 97 MB; removed."
}

Write-Host "  extracting (about 500 MB open)" -ForegroundColor Cyan
tar -xzf $zip -C $Staging
if ($LASTEXITCODE -ne 0) { throw "tar failed on $zip" }

$tsv = Join-Path $Staging 'raw/raw'
if (-not (Test-Path $tsv)) { throw "expected raw/raw inside the archive; not found in $Staging" }

# ── Column two, filtered and deduplicated ────────────────────────────────────────────────────
#
# StreamReader rather than Get-Content: 2.8M lines through the pipeline takes minutes, this takes
# seconds. UTF-8 is explicit because the console here is cp932 and the default would mangle every
# kana in the file.
Write-Host "  splitting the Japanese column" -ForegroundColor Cyan

# Checked here, next to the loop that reads it, because that is the only place a check is worth
# anything — see the note on Assert-Markers.
Assert-Markers

$utf8 = New-Object System.Text.UTF8Encoding($false)
$reader = New-Object System.IO.StreamReader($tsv, $utf8)
$writeAll = New-Object System.IO.StreamWriter($sentences, $false, $utf8)
$writeCasual = New-Object System.IO.StreamWriter($casual, $false, $utf8)
$seen = New-Object 'System.Collections.Generic.HashSet[string]'

$read = 0
$kept = 0
$casualKept = 0

try {
    while ($null -ne ($line = $reader.ReadLine())) {
        $read++
        $tab = $line.IndexOf("`t")
        if ($tab -lt 0) { continue }

        $ja = $line.Substring($tab + 1).Trim()
        if ($ja.Length -lt $MinChars -or $ja.Length -gt $MaxChars) { continue }

        # At least a few kana or kanji, and mostly not latin. The crawl leaves behind untranslated
        # English rows, romaji, and lines that are pure punctuation; every one would be a
        # segmentation disagreement about nothing.
        $japanese = 0
        foreach ($c in $ja.ToCharArray()) {
            $n = [int]$c
            if (($n -ge 0x3040 -and $n -le 0x30FF) -or ($n -ge 0x4E00 -and $n -le 0x9FFF)) { $japanese++ }
        }
        if ($japanese -lt $MinChars) { continue }
        if (($japanese / $ja.Length) -lt 0.5) { continue }

        # Subtitles repeat heavily — the same stock line appears across hundreds of films, and
        # keeping the duplicates would weight it that many times in every rate computed here.
        if (-not $seen.Add($ja)) { continue }

        $writeAll.WriteLine($ja)
        $kept++

        foreach ($marker in $MARKERS) {
            if ($ja.Contains($marker)) {
                $writeCasual.WriteLine($ja)
                $casualKept++
                break
            }
        }
    }
} finally {
    $reader.Dispose()
    $writeAll.Dispose()
    $writeCasual.Dispose()
}

# **The result is checked, not just the inputs.** Both failures this script has had were silent
# and produced exactly this: a full sentence file beside an empty casual one. Roughly a sixth of
# JESC carries one of these markers, so a corpus of subtitle Japanese with none in it is not a
# corpus of subtitle Japanese, and the run should say so rather than leave a plausible zero.
if ($kept -gt 10000 -and $casualKept -eq 0) {
    throw "$kept sentences kept and not one carries a casual marker, which cannot be true of subtitle Japanese. The marker matching is broken; $casual is not usable."
}

# The extracted TSV is half a gigabyte and nothing reads it again.
Remove-Item (Join-Path $Staging 'raw') -Recurse -Force
Remove-Item $zip -Force

# The terms travel with the data rather than being left to a reader of this script.
@'
JESC — Japanese-English Subtitle Corpus
https://nlp.stanford.edu/projects/jesc/
https://arxiv.org/abs/1710.10639

Licensed CC BY-SA 4.0. Pryzant, Chung, Jurafsky and Britz — Stanford University,
Google Brain and Rakuten Institute of Technology. Crawled from movie and television
subtitles and automatically aligned.

Only the Japanese column is kept here. The corpus is NOT gold: no boundary or reading
annotation, and the crawled Japanese is of uneven quality. Usable to measure agreement
between analyzers, never as ground truth.

Fetched by scripts/fetch-jesc.ps1. Not committed — see project notes section 8.
'@ | Set-Content -Path (Join-Path $Out 'ATTRIBUTION.txt') -Encoding utf8

Write-Host ""
Write-Host "  $read pairs read, $kept kept after filtering and deduplication" -ForegroundColor Green
Write-Host "  $casualKept of those carry a casual marker" -ForegroundColor Green
Write-Host ""
Write-Host "  unbiased profile:  `$env:SAKATSU_ANALYZER_SENTENCES='$sentences'"
Write-Host "  targeted mining:   `$env:SAKATSU_ANALYZER_SENTENCES='$casual'"
Write-Host "  cap the run with:  `$env:SAKATSU_ANALYZER_SAMPLE='2000'   # ~1 h; Ichiran is 1.9 s a line here"
