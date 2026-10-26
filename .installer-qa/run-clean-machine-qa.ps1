<#
.SYNOPSIS
    Installs the built installer on a disposable clean machine and reports what happened.

.DESCRIPTION
    Boots Windows Sandbox, installs silently, and asserts outcomes: exit code, WebView2 arriving
    without network, the Add/Remove entry, the executable, and an actual window with a real title.

    **The offline case is the point.** Pass -NoNetwork to prove the installer needs no connection.
    The bug this was written after - `Error 1722`, exit 1603, nothing installed - only appeared on
    a machine that could not reach Microsoft's CDN, and the previous harness could not see it
    because it drove the wizard rather than reading a log.

    Nothing is installed on the host. The sandbox is destroyed on exit.

.PARAMETER NoNetwork
    Disable sandbox networking. The strongest form of the test.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File .\.installer-qa\run-clean-machine-qa.ps1 -NoNetwork
#>
[CmdletBinding()]
param(
    [switch] $NoNetwork,
    [int] $TimeoutSeconds = 900
)
$ErrorActionPreference = 'Stop'

$qa = $PSScriptRoot
$root = Split-Path -Parent $qa
$bundle = Join-Path $root 'target\release\bundle'
if (-not (Test-Path $bundle)) {
    throw "No bundle at $bundle. Run ``cargo tauri build --bundles nsis`` first."
}

$networking = 'Default'
if ($NoNetwork) { $networking = 'Disable' }

# **A unique port is not needed here, but a unique results directory is** - two runs writing to
# one directory is how a stale report gets read as a fresh one.
$before = @()
if (Test-Path (Join-Path $qa 'results')) {
    $before = @(Get-ChildItem (Join-Path $qa 'results') -Directory | Select-Object -ExpandProperty Name)
}

$wsb = Join-Path $env:TEMP 'sakatsu-clean-machine.wsb'
@"
<Configuration>
  <Networking>$networking</Networking>
  <MappedFolders>
    <MappedFolder>
      <HostFolder>$bundle</HostFolder>
      <SandboxFolder>C:\Installers</SandboxFolder>
      <ReadOnly>true</ReadOnly>
    </MappedFolder>
    <MappedFolder>
      <HostFolder>$qa</HostFolder>
      <SandboxFolder>C:\QA</SandboxFolder>
      <ReadOnly>false</ReadOnly>
    </MappedFolder>
  </MappedFolders>
  <LogonCommand>
    <Command>powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\QA\clean-machine-qa.ps1</Command>
  </LogonCommand>
</Configuration>
"@ | Set-Content -Path $wsb -Encoding UTF8

Write-Output "Clean-machine QA: networking=$networking"
Start-Process -FilePath 'WindowsSandbox.exe' -ArgumentList "`"$wsb`""

$deadline = (Get-Date).AddSeconds($TimeoutSeconds)
$run = $null
while ((Get-Date) -lt $deadline) {
    Start-Sleep -Seconds 5
    $now = @(Get-ChildItem (Join-Path $qa 'results') -Directory -ErrorAction SilentlyContinue |
        Where-Object { $before -notcontains $_.Name })
    foreach ($candidate in $now) {
        if (Test-Path (Join-Path $candidate.FullName 'DONE')) { $run = $candidate; break }
    }
    if ($run) { break }
}

Get-Process -Name 'WindowsSandboxClient', 'WindowsSandbox', 'WindowsSandboxRemoteSession' `
    -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue

if (-not $run) { throw "The sandbox did not finish within $TimeoutSeconds seconds." }

Get-Content (Join-Path $run.FullName 'findings.txt')

$checks = Get-Content (Join-Path $run.FullName 'checks.json') -Raw | ConvertFrom-Json
$failed = @($checks | Where-Object { -not $_.passed })
Write-Output ''
Write-Output "Results: $($run.FullName)"
if ($failed.Count -gt 0) {
    throw "$($failed.Count) of $(@($checks).Count) checks failed: $(($failed | ForEach-Object { $_.name }) -join '; ')"
}
Write-Output "All $(@($checks).Count) checks passed."
