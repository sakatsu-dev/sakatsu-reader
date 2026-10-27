<#
.SYNOPSIS
    Fetch UD_Japanese-GSD — the first ground truth this project has for segmentation.

.DESCRIPTION
    ## What it is for

    Every cross-analyzer number in `local documentation` measures **agreement**, and
    `core/tests/analyzer_corpus.rs` says so in its own header: nobody has annotated token
    boundaries for the corpus, so nothing can be right or wrong there, only different. Two
    analyzers agreeing at 96% may be wrong the same way and this cannot tell.

    UD_Japanese-GSD is manually annotated, so it can. Three layers matter here, and all three
    ride on the same tokens:

      SUW boundaries    manual  whether a split is correct, not merely different
      LUWBILabel        B / I   whether 不快 | 感 belong together, per token
      XPOS              manual  動詞-一般-五段-サ行, 助動詞-助動詞-タ — the conjugation class
                                the built-in analyzer rebuilds from suffix tokens

    The LUW layer is the one that unblocks rejoining. Measured over 1,858 sentences, 13.2% of
    Ichiran's spans are ones the built-in analyzer splits finer, and the dictionary knows the
    whole word in 69.9% of them — but knowing a word is no reason to rejoin it, because あの人
    and ある日 are also words the dictionary knows and a learner wants those split. `LUWBILabel`
    is that judgement, made by annotators, token by token.

    ## Only GSD, not GSDLUW

    The long-unit-word treebank is published separately, and it is not needed: GSD carries LUW
    as an annotation layer on its own tokens, so one download answers both granularities. Fetch
    GSDLUW only if the separate tokenisation itself is ever wanted.

    ## What it cannot do

    Its readings come from UniDic — the `UnidicInfo` field — so it **cannot adjudicate readings
    against UniDic**. 私 わたくし is UniDic's answer and GSD repeats it; the suspect does not get
    to judge itself. See `modern_reading` in `core/src/analyzer/lindera/mod.rs`.

    Its register is news and blogs. It validates the mechanism, not this app's domain, and a
    disagreement profile taken here does not transfer to manga or game dialogue.

    ## Licence

    CC BY-SA 4.0 — the same licence already carried for Jitendex, JMnedict and KANJIDIC2, so
    no new obligation in kind is taken on. Attribution belongs in `project notes` §8 beside them.
    Fetched, never committed.

.PARAMETER Out
    Where to write. Defaults beside the other fetched data.

.PARAMETER Splits
    Which splits to take. `test` and `dev` are 3 MB each and enough to start; `train` is 40 MB
    and only earns its place once something is being fitted rather than measured.
#>
[CmdletBinding()]
param(
    [string]$Out = (Join-Path $env:APPDATA 'com.sakatsu.app\corpora\ud-japanese-gsd'),
    [ValidateSet('train', 'dev', 'test')]
    [string[]]$Splits = @('test', 'dev')
)

$ErrorActionPreference = 'Stop'

$BASE = 'https://raw.githubusercontent.com/UniversalDependencies/UD_Japanese-GSD/master'

New-Item -ItemType Directory -Force -Path $Out | Out-Null

foreach ($split in $Splits) {
    $name = "ja_gsd-ud-$split.conllu"
    $destination = Join-Path $Out $name

    if (Test-Path $destination) {
        $size = [math]::Round((Get-Item $destination).Length / 1MB, 1)
        Write-Host "  $name already here ($size MB)" -ForegroundColor DarkGray
        continue
    }

    Write-Host "  fetching $name" -ForegroundColor Cyan
    Invoke-WebRequest -Uri "$BASE/$name" -OutFile $destination -UseBasicParsing

    # A truncated download is worse than a missing one: it parses, it just stops early, and
    # the run that reads it reports a smaller corpus rather than an error.
    $lines = (Get-Content $destination -TotalCount 1)
    if (-not ($lines -match '^# newdoc id')) {
        Remove-Item $destination -Force
        throw "$name did not begin with a CoNLL-U document header; removed."
    }

    $size = [math]::Round((Get-Item $destination).Length / 1MB, 1)
    Write-Host "    $size MB" -ForegroundColor DarkGray
}

# The attribution travels with the data, so it is written beside it rather than left to a
# reader of this script.
$notice = Join-Path $Out 'ATTRIBUTION.txt'
@'
UD_Japanese-GSD
https://github.com/UniversalDependencies/UD_Japanese-GSD

Licensed CC BY-SA 4.0. Japanese data from the Google UDT 2.0, with SUW/LUW and
UniDic annotation layers added by the Universal Dependencies Japanese team.

Fetched by scripts/fetch-ud-japanese.ps1. Not committed — see project notes section 8.
'@ | Set-Content -Path $notice -Encoding utf8

Write-Host "`n  $Out" -ForegroundColor Green
Write-Host "  read it with SAKATSU_UD_GSD=$Out"
