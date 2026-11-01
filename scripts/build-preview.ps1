#requires -Version 7
<# Build and inspect an unsigned private preview. Installer QA requires a disposable Windows runner. #>
[CmdletBinding()]
param(
    [Parameter(Mandatory)] [ValidatePattern('^[0-9a-f]{40}$')] [string] $SourceRevision,
    [Parameter(Mandatory)] [string] $OutputDirectory,
    [Parameter(Mandatory)] [string] $BuildDirectory,
    [switch] $VerifyOnly,
    [switch] $DisposableEnvironment
)
$ErrorActionPreference = 'Stop'
if (-not $DisposableEnvironment) { throw 'This script installs and uninstalls the result. Use a disposable Windows runner.' }
$root = Split-Path -Parent $PSScriptRoot
$savedLocation = Get-Location
$names = @('CARGO_TARGET_DIR', 'RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'CARGO_INCREMENTAL', 'CARGO_PROFILE_DEV_DEBUG', 'CARGO_BUILD_JOBS')
$saved = @{}
foreach ($name in $names) { $saved[$name] = [Environment]::GetEnvironmentVariable($name, 'Process') }
function Checked([string]$Program, [string[]]$Arguments) {
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Program exited with $LASTEXITCODE" }
}
try {
    Set-Location $root
    if ((git rev-parse HEAD).Trim() -ne $SourceRevision) { throw 'Checkout does not match the approved build revision' }
    if ($LASTEXITCODE) { throw 'Could not read source identity' }
    $env:CARGO_TARGET_DIR = [IO.Path]::GetFullPath($BuildDirectory)
    $env:CARGO_INCREMENTAL = '0'
    $env:CARGO_PROFILE_DEV_DEBUG = '0'
    $env:CARGO_BUILD_JOBS = '2'
    $cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
    $env:RUSTFLAGS = $null
    $env:CARGO_ENCODED_RUSTFLAGS = @("--remap-path-prefix=$root=.", "--remap-path-prefix=$cargoHome=cargo",
        "--remap-path-prefix=$env:CARGO_TARGET_DIR=build", "--remap-path-prefix=$env:USERPROFILE=user") -join [char]31
    $work = Join-Path $root 'target/preview-work'
    if (Test-Path -LiteralPath $work) { throw 'Preview work directory already exists; use a fresh checkout' }
    New-Item -ItemType Directory -Path $work -Force | Out-Null
    $pin = Get-Content -LiteralPath scripts/webview2-runtime.json -Raw | ConvertFrom-Json
    $runtime = Join-Path $root ('src-tauri/' + $pin.directory)
    Checked python @('scripts/prepare-webview.py', '--destination', $runtime, '--cache', '.cache/webview', '--download')
    $signature = Get-AuthenticodeSignature -LiteralPath (Join-Path $runtime 'msedgewebview2.exe')
    if ($signature.Status -ne 'Valid' -or -not $signature.TimeStamperCertificate -or
        $signature.SignerCertificate.Subject -notmatch 'O=Microsoft Corporation(?:,|$)') {
        throw 'The pinned Microsoft runtime signature or timestamp is invalid'
    }
    $config = Join-Path $root 'target/preview-build-config.json'
    @{ bundle = @{ createUpdaterArtifacts = $false; windows = @{ webviewInstallMode = @{ type = 'fixedRuntime'; path = $pin.directory } } } } |
        ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $config -Encoding utf8NoBOM
    Checked node @('scripts/stage-vc-runtime.mjs')
    Checked python @('scripts/prepare-essentials.py', '--fetch-missing')
    Checked npm @('--prefix', 'ui', 'ci')
    Checked cargo @('tauri', 'build', '--no-bundle', '--config', $config, '--', '--locked', '--no-default-features', '--bin', 'sakatsu-desktop')
    Checked cargo @('tauri', 'bundle', '--bundles', 'nsis', '--config', $config)
    $installers = @(Get-ChildItem -LiteralPath (Join-Path $env:CARGO_TARGET_DIR 'release/bundle/nsis') -Filter '*-setup.exe')
    if ($installers.Count -ne 1) { throw 'Expected exactly one preview installer' }
    $installer = $installers[0].FullName
    $payload = Join-Path $work 'payload'
    Checked 7z @('x', '-y', '-bso0', $installer, "-o$payload")
    $qa = Join-Path $work 'installer-qa.json'
    # No runtime override: QA must use the same app-local runtime a reader receives.
    & (Join-Path $PSScriptRoot 'test-release-installer.ps1') -Installer $installer -DisposableEnvironment -ResultFile $qa
    $arguments = @('scripts/release-control.py', 'package-preview', '--installer', $installer, '--payload', $payload,
        '--built-app', (Join-Path $env:CARGO_TARGET_DIR 'release/sakatsu-desktop.exe'), '--qa', $qa,
        '--sha', $SourceRevision, '--output', [IO.Path]::GetFullPath($OutputDirectory))
    if ($VerifyOnly) { $arguments += '--verify-only' }
    Checked python $arguments
} finally {
    foreach ($name in $names) { [Environment]::SetEnvironmentVariable($name, $saved[$name], 'Process') }
    Set-Location $savedLocation
}
