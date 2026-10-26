<#
.SYNOPSIS
    Fetch KWDLC — gold segmentation that does *not* come from UniDic.

.DESCRIPTION
    ## What it is for

    `scripts/fetch-ud-japanese.ps1` brought in UD_Japanese-GSD, which is the only ground truth
    this project has for segmentation — and it cannot settle the one comparison that matters.
    GSD's lemmas and POS are UniDic-derived and its short-unit-word standard *is* UniDic's, so
    scoring the built-in analyzer (UniDic) against Ichiran (JMdict) on it asks "which agrees with
    UniDic more" and answers itself. Measured: the built-in analyzer scores SUW F1 77.7 against
    Ichiran's 63.1 on the same 300 sentences, a gap too large to believe.

    KWDLC is annotated in the **JUMAN** lineage instead — a different dictionary family — so it
    can judge between them. It carries readings as well as boundaries, which also makes it the
    first source able to adjudicate 私 わたし / わたくし, a dispute GSD cannot touch because UniDic
    is the suspect.

    ~5,000 web documents, ~15,000 sentences: blogs, commercial pages, news, encyclopedic. Not
    manga or game dialogue, but nearer to it than GSD's newspapers.

    ## THE LICENCE IS UNDECLARED, AND THAT IS THE POINT OF THIS PARAGRAPH

    There is no LICENSE file in the repository and GitHub reports no licence. The README says
    only that the corpus is "released for the purpose of contributing to the research of natural
    language processing" — a statement of intent, not a grant of rights. Every other entry in
    `project notes` §8 names a licence; this one cannot.

    So it is treated the way §8 treats Imabi, Tae Kim and Pomax: **consult, never import.**

      - Fetched to the user's data directory, outside the repository entirely — not merely
        gitignored, so there is no path by which it can be committed.
      - Used to *measure*. No text, reading or boundary from it is copied into a pack, a
        fixture, a dictionary or a correction table.
      - Nothing derived from it ships.

    If that is not enough for you, ask the authors at https://nlp.ist.i.kyoto-u.ac.jp/EN/?KWDLC
    before going further. This script deliberately does not pretend the question is settled.

.PARAMETER Out
    Where to write. Defaults beside the other fetched corpora.
#>
[CmdletBinding()]
param(
    [string]$Out = (Join-Path $env:APPDATA 'com.sakatsu.app\corpora\kwdlc')
)

$ErrorActionPreference = 'Stop'

$ARCHIVE = 'https://github.com/ku-nlp/KWDLC/archive/refs/heads/master.zip'

if (Test-Path (Join-Path $Out 'knp')) {
    $files = (Get-ChildItem -Path (Join-Path $Out 'knp') -Recurse -File).Count
    Write-Host "  already here: $files annotated files in $Out" -ForegroundColor DarkGray
    return
}

New-Item -ItemType Directory -Force -Path $Out | Out-Null
$zip = Join-Path $env:TEMP 'kwdlc-master.zip'

Write-Host "  fetching KWDLC (about 30 MB)" -ForegroundColor Cyan
Invoke-WebRequest -Uri $ARCHIVE -OutFile $zip -UseBasicParsing

$staging = Join-Path $env:TEMP 'kwdlc-staging'
if (Test-Path $staging) { Remove-Item $staging -Recurse -Force }
Expand-Archive -Path $zip -DestinationPath $staging -Force

# Only the annotation and the ids. `org/` is the raw web text and nothing here reads it.
$root = Get-ChildItem -Path $staging -Directory | Select-Object -First 1
foreach ($part in @('knp', 'id')) {
    $source = Join-Path $root.FullName $part
    if (Test-Path $source) {
        Copy-Item -Path $source -Destination $Out -Recurse -Force
    }
}
Copy-Item -Path (Join-Path $root.FullName 'README.md') -Destination (Join-Path $Out 'README.md') -Force

Remove-Item $zip -Force
Remove-Item $staging -Recurse -Force

# The terms travel with the data, and here that means recording that there are none.
@'
KWDLC — Kyoto University Web Document Leads Corpus
https://github.com/ku-nlp/KWDLC
https://nlp.ist.i.kyoto-u.ac.jp/EN/?KWDLC

NO LICENCE IS DECLARED. The repository carries no LICENSE file and GitHub reports
none. The README states only that the corpus is "released for the purpose of
contributing to the research of natural language processing".

Treated here as consult-never-import, the same standing project notes section 8 gives
Imabi and Tae Kim: used to measure, never copied into anything that ships.

Fetched by scripts/fetch-kwdlc.ps1. Not committed.
'@ | Set-Content -Path (Join-Path $Out 'ATTRIBUTION.txt') -Encoding utf8

$files = (Get-ChildItem -Path (Join-Path $Out 'knp') -Recurse -File).Count
Write-Host "`n  $files annotated files in $Out" -ForegroundColor Green
Write-Host "  read it with SAKATSU_KWDLC=$Out"
