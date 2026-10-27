<#
.SYNOPSIS
    Run everything, including the tests `cargo test` leaves out — and say which ones
    actually executed.

.DESCRIPTION
    `cargo test --workspace` reports every binary green while thirty-odd `#[ignore]`d
    tests sit unexecuted. Two of them had been broken for months before anyone ran them:
    a readiness assertion left stale when PP-OCRv5 split one panel row into two, and a
    probe that died inside the `tokenizers` crate. Both were invisible behind a green tick.

    ## The part cargo cannot do

    Running the ignored suite is only half of it. Most of those tests check for what they
    need and return early when it is missing —

        no imported Jitendex — skipped
        ichiran-cli is not answering — skipped
        no corpus at ... — nothing to score

    — and cargo reports every one of those as **ok**. A machine with no models, no
    dictionaries and no sidecar shows a wall of green while testing almost nothing.

    So this separates *passed* from *skipped*, by looking for the messages those tests
    print. That is string matching on text this repository owns, so it can drift: every
    matched line is printed verbatim rather than counted, because seeing what was matched
    is what makes drift obvious instead of silently under-reporting — which is the exact
    failure being fixed here.

    A suite whose ignored tests all say that `--release` is mandatory is reported as
    skipped here. Those files are performance/evaluation jobs, not debug tests; running
    them in the debug profile can turn a minutes-long measurement into an hours-long one.

.PARAMETER IgnoredOnly
    Skip the ordinary suite and run only the ignored one.

.EXAMPLE
    pwsh -File scripts/test-all.ps1
    pwsh -File scripts/test-all.ps1 -IgnoredOnly
#>
[CmdletBinding()]
param(
    [switch]$IgnoredOnly
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root

# One token, not a list of guesses.
#
# The first version of this matched nine phrases lifted from the test sources and missed
# `vlm_live` on its very first run: that file prints "nothing answering at ..." and the
# list had "is not answering". A tenth pattern would have deferred the same miss to the
# eleventh message somebody adds — and a skip detector that under-reports is precisely the
# green tick this script exists to replace.
#
# So the tests were changed instead: every early return now prints `SKIP: <reason>`. One
# marker, matched exactly, and a convention plain enough that the next one copies it.
$SKIP_MARKER = 'SKIP:'

function Test-Binaries {
    foreach ($pair in @(@('sakatsu-core', 'core'), @('sakatsu-desktop', 'src-tauri'))) {
        # Ignored unit tests live in the library harness, not under `tests/`. Leaving this
        # out made the script's "everything" claim false for the real Ichiran, ichi.moe,
        # and audio checks even while its summary looked complete.
        [pscustomobject]@{
            Package = $pair[0]
            Name = 'lib'
            IsLibrary = $true
            ReleaseOnly = $false
        }

        $dir = Join-Path $root (Join-Path $pair[1] 'tests')
        if (-not (Test-Path $dir)) { continue }
        foreach ($file in Get-ChildItem $dir -Filter *.rs -File) {
            $source = Get-Content $file.FullName -Raw
            $ignored = [regex]::Matches($source, '#\[ignore(?:\s*=\s*"[^"]*")?\]')
            $release = [regex]::Matches(
                $source,
                '#\[ignore\s*=\s*"[^"]*--release[^"]*"\]'
            )
            [pscustomobject]@{
                Package = $pair[0]
                Name = $file.BaseName
                IsLibrary = $false
                ReleaseOnly = ($ignored.Count -gt 0 -and $ignored.Count -eq $release.Count)
            }
        }
    }
}

$failed = [System.Collections.Generic.List[string]]::new()

# Windows PowerShell 5 wraps a native program's stderr as NativeCommandError records. With the
# script-wide Stop preference, ordinary Cargo progress (which is written to stderr) terminates the
# verifier before its real exit code can be inspected. PowerShell 7 does not normally expose that
# trap, but this repository supports Windows machines where only the in-box host is available.
function Invoke-CargoCaptured {
    param([string[]]$Arguments)

    $previousPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $text = & cargo @Arguments 2>&1 | Out-String
        $code = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previousPreference
    }
    [pscustomobject]@{ Output = $text; ExitCode = $code }
}

# ---------------------------------------------------------------------------------------
# Pass one: what CI runs.
# ---------------------------------------------------------------------------------------
if (-not $IgnoredOnly) {
    Write-Host "=== the ordinary suite ===" -ForegroundColor Cyan
    $run = Invoke-CargoCaptured -Arguments @('test', '--workspace')
    $output = $run.Output
    $ok = ($output | Select-String 'test result: ok' -AllMatches).Matches.Count
    if ($run.ExitCode -ne 0) {
        Write-Host "  FAILED" -ForegroundColor Red
        $output -split "`n" | Where-Object { $_ -match 'FAILED|panicked at|^error' } |
            Select-Object -First 12 | ForEach-Object { "    $_" }
        $failed.Add('cargo test --workspace')
    } else {
        Write-Host "  $ok binaries green" -ForegroundColor Green
    }
    Write-Host ""
}

# ---------------------------------------------------------------------------------------
# Pass two: the ignored suite, one binary at a time so a failure does not hide the rest.
# ---------------------------------------------------------------------------------------
Write-Host "=== the ignored suite ===" -ForegroundColor Cyan
Write-Host "(what needs models, dictionaries, a sidecar or a server)`n"

$ran = 0
$skipped = 0

foreach ($suite in Test-Binaries) {
    if ($suite.ReleaseOnly) {
        Write-Host ("{0,-22} SKIPPED" -f $suite.Name) -ForegroundColor Yellow
        Write-Host "    SKIP: every ignored test in this suite explicitly requires --release" `
            -ForegroundColor DarkYellow
        $skipped++
        continue
    }

    $cargoArgs = @('test', '-p', $suite.Package)
    if ($suite.IsLibrary) {
        $cargoArgs += '--lib'
    } else {
        $cargoArgs += @('--test', $suite.Name)
    }
    $cargoArgs += @('--', '--ignored', '--nocapture')
    $run = Invoke-CargoCaptured -Arguments $cargoArgs
    $output = $run.Output
    $result = [regex]::Match($output, 'test result: (\w+)\. (\d+) passed; (\d+) failed')

    if (-not $result.Success) {
        Write-Host ("{0,-22} could not run" -f $suite.Name) -ForegroundColor Red
        $output -split "`n" | Where-Object { $_ -match '^error' } | Select-Object -First 3 |
            ForEach-Object { "    $_" }
        $failed.Add($suite.Name)
        continue
    }

    $passed = [int]$result.Groups[2].Value
    $failedCount = [int]$result.Groups[3].Value

    $skips = $output -split "`n" |
        Where-Object { $_ -like "*$SKIP_MARKER*" } |
        ForEach-Object { $_.Trim() } |
        Select-Object -Unique

    if ($failedCount -gt 0) {
        Write-Host ("{0,-22} {1} FAILED" -f $suite.Name, $failedCount) -ForegroundColor Red
        $output -split "`n" | Where-Object { $_ -match 'panicked at|^test .* FAILED' } |
            Select-Object -First 6 | ForEach-Object { "    $($_.Trim())" }
        $failed.Add($suite.Name)
    } elseif ($skips) {
        # The case cargo calls "ok" and this exists to contradict.
        Write-Host ("{0,-22} SKIPPED" -f $suite.Name) -ForegroundColor Yellow
        foreach ($line in $skips) { Write-Host "    $line" -ForegroundColor DarkYellow }
        $skipped++
    } elseif ($passed -eq 0) {
        Write-Host ("{0,-22} no ignored tests" -f $suite.Name) -ForegroundColor DarkGray
    } else {
        Write-Host ("{0,-22} {1} passed" -f $suite.Name, $passed) -ForegroundColor Green
        $ran++
    }
}

Write-Host ""
Write-Host "=== summary ===" -ForegroundColor Cyan
Write-Host "  ran for real   $ran suite(s)"
Write-Host "  skipped        $skipped suite(s) — something they need is not on this machine"
if ($failed.Count -gt 0) {
    Write-Host "  FAILED         $($failed -join ', ')" -ForegroundColor Red
}

Pop-Location
if ($failed.Count -gt 0) { exit 1 }
