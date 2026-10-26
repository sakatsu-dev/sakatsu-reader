<#
.SYNOPSIS
    Checks for the checkpoint launcher's preparation and reuse behaviour.

.DESCRIPTION
    `cargo fmt` and `clippy` say nothing about PowerShell, and these are behaviours worth checking:
    a reuse that reseeds destroys the state a restart row exists to observe, and an unvalidated
    -Reuse argument reaches Remove-Item -Recurse.

    Run:
        pwsh -File scripts/desktop-checkpoint.Tests.ps1

    Every case uses -PrepareOnly, so nothing is launched and no window opens. Resource copying is
    skipped by pointing the launcher at profiles it has already prepared, except for one case that
    prepares a real one -- that case copies the dictionaries and is the slow one.
#>
#requires -Version 5.1
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$launcher = Join-Path $PSScriptRoot 'desktop-checkpoint.ps1'
$failures = 0
$prepared = @()

function Test-Case([string]$Name, [scriptblock]$Body) {
    try {
        & $Body
        Write-Host "  ok    $Name" -ForegroundColor Green
    } catch {
        $script:failures++
        Write-Host "  FAIL  $Name" -ForegroundColor Red
        Write-Host "        $($_.Exception.Message)"
    }
}

function Invoke-Launcher([string[]]$LauncherArgs) {
    $output = & pwsh -NoProfile -File $launcher @LauncherArgs 2>&1 | Out-String
    return [pscustomobject]@{ Output = $output; ExitCode = $LASTEXITCODE }
}

function Assert-True([bool]$Condition, [string]$Because) {
    if (-not $Condition) { throw $Because }
}

Write-Host 'checkpoint launcher checks'
Write-Host ''

# --- Argument validation, which costs nothing to check and guards a recursive delete -----------
Test-Case '-Reuse refuses a path rather than a profile name' {
    $result = Invoke-Launcher @('-Reuse', 'C:\Windows', '-PrepareOnly')
    Assert-True ($result.ExitCode -ne 0) 'it should have failed'
    Assert-True ($result.Output -match "takes the profile's name") "expected the name complaint, got: $($result.Output)"
}

Test-Case '-Reuse refuses a directory that is not a checkpoint profile' {
    $stranger = Join-Path ([System.IO.Path]::GetTempPath()) "not-a-checkpoint-$PID"
    New-Item -ItemType Directory -Force -Path $stranger | Out-Null
    New-Item -ItemType File -Force -Path (Join-Path $stranger 'something-precious.txt') | Out-Null
    try {
        $result = Invoke-Launcher @('-Reuse', (Split-Path -Leaf $stranger), '-PrepareOnly')
        Assert-True ($result.ExitCode -ne 0) 'it should have refused'
        Assert-True ($result.Output -match 'is not a checkpoint profile') "expected the marker complaint, got: $($result.Output)"
        Assert-True (Test-Path (Join-Path $stranger 'something-precious.txt')) 'and it must not have removed anything'
    } finally {
        Remove-Item -Recurse -Force $stranger -ErrorAction SilentlyContinue
    }
}

Test-Case '-Scenario rejects a value it does not know' {
    $result = Invoke-Launcher @('-Scenario', 'whatever', '-PrepareOnly')
    Assert-True ($result.ExitCode -ne 0) 'it should have failed'
}

# --- Preparation, once, and then reuse against it ----------------------------------------------
$profileName = $null
$prepareOutput = $null
Test-Case 'preparing a recovery profile plants four containers and a marker' {
    $result = Invoke-Launcher @('-Scenario', 'recovery', '-Analyzer', 'lindera', '-PrepareOnly')
    $script:prepareOutput = $result.Output
    Assert-True ($result.ExitCode -eq 0) "preparation failed: $($result.Output)"
    if ($result.Output -match 'profile kept: (.+)') {
        $script:profileName = Split-Path -Leaf $Matches[1].Trim()
        $script:prepared += $Matches[1].Trim()
    }
    Assert-True ($null -ne $script:profileName) "no profile was reported: $($result.Output)"
    $marker = Join-Path ([System.IO.Path]::GetTempPath()) $script:profileName 'checkpoint-profile.json'
    Assert-True (Test-Path $marker) 'the profile is not marked'
    $recorded = Get-Content -Raw $marker | ConvertFrom-Json
    Assert-True ($recorded.scenario -eq 'recovery') "the marker says $($recorded.scenario)"
    Assert-True ($recorded.analyzer -eq 'lindera') "the marker says analyzer '$($recorded.analyzer)'"
    Assert-True ($result.Output -match 'checkpoint-stale') 'the stale row has no container of its own'
    Assert-True ($result.Output -match 'Unresolved') 'nothing is outstanding'
}

# --- The printed checklist against the fixture that was actually planted -----------------------
#
# **This is the check that was missing.** The suite verified the fixture output and never the
# guidance printed beside it, so the checklist went on saying "Three entries" and directing the
# stale row at checkpoint-keep -- the row-2/row-3 collision -- long after the fixture had four
# containers with distinct words. A person following it would have consumed the stale row's own
# transaction and searched for words no container holds.
#
# Both halves come from the one prepare run above, so this costs no extra profile.
Test-Case 'the printed checklist matches the fixture that was planted' {
    Assert-True ($null -ne $prepareOutput) 'no prepare output was captured'

    # The fixture prints one line per container: "  <name>   previous: <word>   current: <word>"
    $planted = [regex]::Matches(
        $prepareOutput,
        '(?m)^\s+(checkpoint-[a-z]+)\s+previous:\s*(\S+)\s+current:\s*(\S+)\s*$')
    Assert-True ($planted.Count -ge 4) "expected at least four planted containers, saw $($planted.Count)"

    foreach ($match in $planted) {
        $container = $match.Groups[1].Value
        Assert-True ($prepareOutput -match [regex]::Escape($container)) `
            "the checklist never names $container"
    }

    # The count the checklist claims must be the count the fixture planted.
    Assert-True ($prepareOutput -match 'R1\. Four entries') `
        'the checklist does not say four entries; it drifted from the fixture'

    # The stale row must act on its own container. Directing it at checkpoint-keep is the
    # collision: R3 resolves that transaction, leaving R5 nothing to act on.
    Assert-True ($prepareOutput -match 'resolve .* checkpoint-stale') `
        'the stale row does not act on checkpoint-stale'
    Assert-True (-not ($prepareOutput -match 'resolve .* checkpoint-keep')) `
        'the stale row still consumes the keep row transaction'

    # The words the checklist tells a person to search must be words some container holds.
    $words = @()
    foreach ($match in $planted) { $words += $match.Groups[2].Value; $words += $match.Groups[3].Value }
    foreach ($searched in 'restorable', 'restoreheld', 'keepheld', 'blockable', 'staleheld') {
        Assert-True ($words -contains $searched) `
            "the checklist tells a person to search '$searched', which no planted container holds"
    }
    # And the retired shared words must be gone from the guidance entirely.
    foreach ($retired in 'checkpoint-previous', 'checkpoint-current') {
        Assert-True (-not ($prepareOutput -match [regex]::Escape($retired))) `
            "the checklist still names the retired shared word '$retired'"
    }
}

Test-Case 'reuse does not reseed the fixtures it is meant to preserve' {
    Assert-True ($null -ne $profileName) 'no prepared profile'
    $dictionaries = Join-Path ([System.IO.Path]::GetTempPath()) $profileName 'Local\com.sakatsu.app\dictionaries'
    # Resolve one container as a person would after acting on it: the record goes away.
    $record = Join-Path $dictionaries '.checkpoint-keep.recovery.json'
    Assert-True (Test-Path $record) 'the fixture is not there to begin with'
    Remove-Item -Force $record

    $result = Invoke-Launcher @('-Reuse', $profileName, '-PrepareOnly')
    Assert-True ($result.ExitCode -eq 0) "reuse failed: $($result.Output)"
    Assert-True (-not (Test-Path $record)) 'reuse replanted the fixture, which is the defect this checks for'
    Assert-True ($result.Output -match 'not reseeding') "reuse should say what it is doing: $($result.Output)"
}

Test-Case 'reuse refuses a scenario the profile was not prepared for' {
    Assert-True ($null -ne $profileName) 'no prepared profile'
    $result = Invoke-Launcher @('-Reuse', $profileName, '-Scenario', 'relocation', '-PrepareOnly')
    Assert-True ($result.ExitCode -ne 0) 'it should have refused'
    Assert-True ($result.Output -match 'prepared for -Scenario recovery') "expected the scenario complaint, got: $($result.Output)"
}

Test-Case 'reuse refuses a different analyzer' {
    Assert-True ($null -ne $profileName) 'no prepared profile'
    $result = Invoke-Launcher @('-Reuse', $profileName, '-Analyzer', 'ichiran', '-PrepareOnly')
    Assert-True ($result.ExitCode -ne 0) 'it should have refused'
    Assert-True ($result.Output -match 'prepared with analyzer') "expected the analyzer complaint, got: $($result.Output)"
}

Test-Case 'a junction pointing out of the profile refuses the launch' {
    Assert-True ($null -ne $profileName) 'no prepared profile'
    # The escape the launcher cannot clear away: a directory inside the profile that points
    # elsewhere. A prefix comparison would also pass this, which is why containment resolves
    # junctions and compares by path component.
    $device = Join-Path ([System.IO.Path]::GetTempPath()) $profileName 'Local\com.sakatsu.app'
    $escaped = Join-Path $device 'wordnet'
    if (Test-Path $escaped) { Remove-Item -Recurse -Force $escaped }
    & cmd /c mklink /J "$escaped" "$env:SystemRoot\Temp" | Out-Null
    try {
        $result = Invoke-Launcher @('-Reuse', $profileName, '-PrepareOnly')
        Assert-True ($result.ExitCode -ne 0) 'it should have refused to launch'
        Assert-True ($result.Output -match 'wordnet resolves to') "expected wordnet to be named, got: $($result.Output)"
    } finally {
        cmd /c rmdir "$escaped" | Out-Null
    }
}

Test-Case 'a sibling directory sharing a name prefix is not treated as inside' {
    # `<profile>-other` starts with `<profile>` as a string and is a different directory. The
    # containment check compares by path component, and this is the case a StartsWith would pass.
    $temp = [System.IO.Path]::GetTempPath()
    $profileDir = Join-Path $temp "sakatsu-prefix-$PID"
    $sibling = "$profileDir-other"
    New-Item -ItemType Directory -Force -Path (Join-Path $profileDir 'Local\com.sakatsu.app') | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path $sibling 'Local\com.sakatsu.app') | Out-Null
    try {
        $env:APPDATA = Join-Path $sibling 'Roaming'
        $env:LOCALAPPDATA = Join-Path $sibling 'Local'
        $env:SAKATSU_DEVICE_DATA_DIR = Join-Path $sibling 'Local\com.sakatsu.app'
        Push-Location $root
        try {
            $out = & cargo run --quiet -p sakatsu-desktop --bin resolve-resources -- --inside $profileDir 2>&1 | Out-String
            $code = $LASTEXITCODE
        } finally {
            Pop-Location
        }
        Assert-True ($code -ne 0) "a sibling sharing the prefix must not count as inside: $out"
        Assert-True ($out -match 'dictionaries resolves to') "and it must name what escaped: $out"
    } finally {
        Remove-Item Env:APPDATA, Env:LOCALAPPDATA, Env:SAKATSU_DEVICE_DATA_DIR -ErrorAction SilentlyContinue
        Remove-Item -Recurse -Force $profileDir, $sibling -ErrorAction SilentlyContinue
    }
}

Test-Case '-PrepareOnly marks its executable identity preliminary' {
    Assert-True ($null -ne $profileName) 'no prepared profile'
    $result = Invoke-Launcher @('-Reuse', $profileName, '-PrepareOnly')
    Assert-True ($result.ExitCode -eq 0) "reuse failed: $($result.Output)"
    Assert-True ($result.Output -match 'PRELIMINARY') "it must say the identity is preliminary: $($result.Output)"
    Assert-True ($result.Output -match 'source\)') 'the interface digest must be labelled as source'
}

Test-Case 'the launcher records the running executable rather than the one on disk' {
    $text = Get-Content -Raw $launcher
    Assert-True ($text -match "Get-Process -Name 'sakatsu-desktop'") 'nothing looks for the running process'
    Assert-True ($text -match 'OBSERVED BUILD IDENTITY') 'nothing reports what actually ran'
    Assert-True ($text -match 'THE INTERFACE SOURCE CHANGED') 'a hot reload during observation would go unrecorded'
}

Test-Case 'a relocation launch leaves the device-data override unset' {
    # The launcher only reaches the environment inside its launch block, so this reads the script:
    # what matters is that the exception exists and names relocation.
    $text = Get-Content -Raw $launcher
    Assert-True ($text -match "if \(\`$Scenario -ne 'relocation'\) \{\s*\`$env:SAKATSU_DEVICE_DATA_DIR") 'the exception is gone'
    Assert-True ($text -match 'Remove-Item Env:SAKATSU_DEVICE_DATA_DIR') 'nothing clears it for relocation'
}

Test-Case 'every resource override is cleared before launching' {
    $text = Get-Content -Raw $launcher
    foreach ($name in 'SAKATSU_DICT_DIR', 'SAKATSU_KANJI_DIR', 'SAKATSU_WORDNET_DIR', 'SAKATSU_AUDIO_DIR', 'SAKATSU_MODEL_DIR', 'SAKATSU_MT_DIR', 'SAKATSU_PADDLE_DIR', 'SAKATSU_VLM_DIR') {
        Assert-True ($text -match [regex]::Escape($name)) "$name is not handled at all"
    }
}

Write-Host ''
foreach ($kept in $prepared) {
    Remove-Item -Recurse -Force $kept -ErrorAction SilentlyContinue
    Write-Host "removed $kept"
}

if ($failures) {
    Write-Host "$failures failed" -ForegroundColor Red
    exit 1
}
Write-Host 'all checks passed' -ForegroundColor Green
exit 0
