<#
.SYNOPSIS
    Run test-release-installer.ps1 inside Windows Sandbox, and bring its result back.

.DESCRIPTION
    Windows Sandbox is a disposable copy of this Windows: whatever the installer registers there
    is gone when the sandbox closes, and the reader's own installation and profile are never
    visible to it. That is the only place on a development machine where the harness may install.

    The sandbox sees, read-only, the folder of each installer and a copy of the two harness
    scripts; it can write only to -OutputDir\out. Networking is disabled: the installer carries
    WebView2 offline, and a release must install without the network. The sandbox shuts itself
    down when the harness finishes, unless -KeepOpen.

    Windows Sandbox runs one instance at a time, so this refuses while one is open.

.EXAMPLE
    pwsh -File scripts/run-release-qa-sandbox.ps1 -Installer new-setup.exe -PreviousInstaller old-setup.exe -OutputDir scratch\qa-run
#>
#requires -Version 5.1
[CmdletBinding()]
param(
    [Parameter(Mandatory)] [string] $Installer,
    [string] $PreviousInstaller,
    [Parameter(Mandatory)] [string] $OutputDir,
    [int] $TimeoutMinutes = 40,
    [switch] $KeepOpen
)

$ErrorActionPreference = 'Stop'

$sandbox = Join-Path $env:windir 'System32\WindowsSandbox.exe'
if (-not (Test-Path -LiteralPath $sandbox)) {
    throw 'Windows Sandbox is not enabled on this machine (Windows Features > Windows Sandbox).'
}
$open = @(Get-Process -Name 'WindowsSandbox', 'WindowsSandboxClient', 'WindowsSandboxRemoteSession' -ErrorAction SilentlyContinue)
if ($open.Count -gt 0) {
    throw 'A Windows Sandbox is already open. Close it first: only one can run at a time.'
}

$current = (Resolve-Path -LiteralPath $Installer).Path
$previous = $null
if ($PreviousInstaller) { $previous = (Resolve-Path -LiteralPath $PreviousInstaller).Path }

$output = [System.IO.Path]::GetFullPath($OutputDir)
if ((Test-Path -LiteralPath $output) -and @(Get-ChildItem -LiteralPath $output -Force).Count -gt 0) {
    throw "The output folder must be new or empty, so one run's result cannot be read as another's: $output"
}
$scripts = Join-Path $output 'scripts'
$out = Join-Path $output 'out'
New-Item -ItemType Directory -Force -Path $scripts, $out | Out-Null
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'test-release-installer.ps1') -Destination $scripts
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'release-qa.psm1') -Destination $scripts

# Each installer's folder is mapped read-only; one folder holding both is mapped once.
$mappings = [ordered]@{}
$mappings[[System.IO.Path]::GetDirectoryName($current)] = 'C:\qa\in-1'
if ($previous) {
    $folder = [System.IO.Path]::GetDirectoryName($previous)
    if (-not $mappings.Contains($folder)) { $mappings[$folder] = 'C:\qa\in-2' }
}
function Inside([string]$HostPath) {
    return (Join-Path $mappings[[System.IO.Path]::GetDirectoryName($HostPath)] ([System.IO.Path]::GetFileName($HostPath)))
}

$arguments = "-Installer `"$(Inside $current)`""
if ($previous) { $arguments += " -PreviousInstaller `"$(Inside $previous)`"" }
$finish = if ($KeepOpen) { 'rem -KeepOpen: the sandbox stays open for inspection.' } else { 'shutdown /s /t 15' }
$command = @"
@echo off
powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\qa\scripts\test-release-installer.ps1 $arguments -DisposableEnvironment -SandboxWebView2Runtime -ResultFile C:\qa\out\result.json > C:\qa\out\console.log 2>&1
echo %ERRORLEVEL% > C:\qa\out\exit-code.txt
$finish
"@
[System.IO.File]::WriteAllText((Join-Path $scripts 'run.cmd'), $command.Replace("`n", "`r`n").Replace("`r`r", "`r"), (New-Object System.Text.ASCIIEncoding))

function Escape([string]$Text) { return [System.Security.SecurityElement]::Escape($Text) }
$folders = @()
foreach ($hostFolder in $mappings.Keys) {
    $folders += "    <MappedFolder><HostFolder>$(Escape $hostFolder)</HostFolder><SandboxFolder>$($mappings[$hostFolder])</SandboxFolder><ReadOnly>true</ReadOnly></MappedFolder>"
}
$folders += "    <MappedFolder><HostFolder>$(Escape $scripts)</HostFolder><SandboxFolder>C:\qa\scripts</SandboxFolder><ReadOnly>true</ReadOnly></MappedFolder>"
$folders += "    <MappedFolder><HostFolder>$(Escape $out)</HostFolder><SandboxFolder>C:\qa\out</SandboxFolder><ReadOnly>false</ReadOnly></MappedFolder>"
$configuration = @"
<Configuration>
  <Networking>Disable</Networking>
  <ClipboardRedirection>Disable</ClipboardRedirection>
  <MappedFolders>
$($folders -join "`r`n")
  </MappedFolders>
  <LogonCommand><Command>C:\qa\scripts\run.cmd</Command></LogonCommand>
</Configuration>
"@
$wsb = Join-Path $output 'release-qa.wsb'
[System.IO.File]::WriteAllText($wsb, $configuration, (New-Object System.Text.UTF8Encoding $false))

Write-Output "Starting Windows Sandbox. Result folder: $out"
Start-Process -FilePath $sandbox -ArgumentList "`"$wsb`""
$exitFile = Join-Path $out 'exit-code.txt'
$deadline = (Get-Date).AddMinutes($TimeoutMinutes)
while (-not (Test-Path -LiteralPath $exitFile) -and (Get-Date) -lt $deadline) { Start-Sleep -Seconds 5 }
if (-not (Test-Path -LiteralPath $exitFile)) {
    throw "No result after $TimeoutMinutes minutes. The sandbox is still open; see $out\console.log."
}
Start-Sleep -Seconds 2
$code = [int]((Get-Content -LiteralPath $exitFile -Raw).Trim())
$resultFile = Join-Path $out 'result.json'
if (Test-Path -LiteralPath $resultFile) {
    $result = Get-Content -LiteralPath $resultFile -Raw | ConvertFrom-Json
    Write-Output "Mode: $($result.mode). Passed: $($result.passed)."
    foreach ($step in $result.steps) { Write-Output "  $step" }
    foreach ($note in $result.observations) { Write-Output "  note: $note" }
    if ($result.error) { Write-Output "  error: $($result.error)" }
}
if ($code -ne 0) { throw "The harness failed inside the sandbox (exit $code). See $out\console.log." }
Write-Output 'Sandbox installer QA passed.'
