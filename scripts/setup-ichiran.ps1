<#
.SYNOPSIS
    Checks, installs, and repairs Sakatsu's recommended main analyzer, Ichiran.

.DESCRIPTION
    Sakatsu Reader already analyses Japanese with its built-in Lindera engine. Ichiran
    is the recommended main analyzer, with Lindera assistance and fallback. Setup can
    be deferred while the built-in analyzer is used. Nothing you read leaves the machine.

    Ichiran is a Linux program backed by PostgreSQL. Its supported Docker setup restores
    roughly 4.7 GB of database data, so it cannot reasonably be bundled with the Windows
    installer. This script turns the upstream procedure into a guided operation:

      1. check WSL and Docker before doing expensive work;
      2. preserve a working native Ichiran installation;
      3. clone and start upstream's Docker Compose setup when -Install is given;
      4. install a small per-user wrapper that restarts stopped containers once; and
      5. configure future Sakatsu Reader processes to use that wrapper.

    It never deletes an existing database or repository. A failed database import may
    require manual cleanup, but that destructive operation is deliberately not automated.

.PARAMETER Install
    Perform setup. Without it, the script only reports what is present and what is needed.

.PARAMETER Distribution
    WSL distribution to use. Defaults to the user's current WSL default.

.PARAMETER Guided
    Keep the final result on screen until Enter is pressed. The app uses this mode.

.EXAMPLE
    powershell -File scripts/setup-ichiran.ps1
    powershell -File scripts/setup-ichiran.ps1 -Install
#>
[CmdletBinding()]
param(
    [switch] $Install,
    [string] $Distribution,
    [switch] $Guided,
    # Passed by the app. A file is necessary in addition to the user environment:
    # Tauri's in-app restart can inherit the current process's old environment block.
    [string] $CommandFile
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$script:WslExitCode = 0
# Keep the file itself ASCII so Windows PowerShell 5.1 reads it consistently without a
# UTF-8 BOM. Construct the one Japanese probe character at runtime instead.
$probeText = [char]0x6C34

function Complete-Setup([int] $Code) {
    if ($Guided) {
        Write-Output ""
        $null = Read-Host "Press Enter to close this window"
    }
    exit $Code
}

function Invoke-Wsl([string] $Command) {
    if ($Distribution) {
        $output = (& wsl.exe -d $Distribution -e bash -lc $Command 2>&1)
    }
    else {
        $output = (& wsl.exe -e bash -lc $Command 2>&1)
    }
    # $LASTEXITCODE is local to the function scope. Keep an explicit script-scoped copy
    # so callers never mistake the result of an earlier native command for WSL's result.
    $script:WslExitCode = $LASTEXITCODE
    return $output
}

function Test-Wsl([string] $Command) {
    $null = Invoke-Wsl $Command
    return $script:WslExitCode -eq 0
}

function Analyzer-Command([string] $LinuxProgram) {
    if ($Distribution) {
        return "wsl -d $Distribution -e $LinuxProgram"
    }
    return "wsl -e $LinuxProgram"
}

function Save-AnalyzerCommand([string] $LinuxProgram) {
    $command = Analyzer-Command $LinuxProgram
    [Environment]::SetEnvironmentVariable('SAKATSU_ICHIRAN_CMD', $command, 'User')
    if ($CommandFile) {
        $parent = Split-Path -Parent $CommandFile
        if ($parent) { $null = New-Item -ItemType Directory -Path $parent -Force }

        $prefix = @()
        if ($Distribution) { $prefix += @('-d', $Distribution) }
        $prefix += @('-e', $LinuxProgram, '-f')
        $json = [ordered]@{
            program = 'wsl'
            prefix_args = $prefix
        } | ConvertTo-Json -Compress
        $utf8 = New-Object System.Text.UTF8Encoding($false)
        [IO.File]::WriteAllText($CommandFile, $json, $utf8)
        Write-Output "  saved launcher for Sakatsu Reader: $CommandFile"
    }
    Write-Output "Sakatsu Reader: configured for this Windows account"
    Write-Output "  $command"
}

function Offer-WslInstall {
    Write-Output "WSL is required but is not ready."
    Write-Output ""
    Write-Output "Sakatsu Reader still works with its built-in analyzer. To add Ichiran,"
    Write-Output "Windows must first install WSL and Ubuntu; this needs administrator approval"
    Write-Output "and may ask you to restart the computer."

    if (-not ($Install -and $Guided)) {
        Write-Output ""
        Write-Output "Run as administrator, then restart Windows:"
        Write-Output "  wsl --install -d Ubuntu"
        Complete-Setup 10
    }

    $answer = Read-Host "Ask Windows to install WSL and Ubuntu now? [y/N]"
    if ($answer -notmatch '(?i)^y(?:es)?$') { Complete-Setup 10 }

    $arguments = '-NoProfile -Command "wsl.exe --install -d Ubuntu"'
    try {
        $process = Start-Process powershell.exe -Verb RunAs -ArgumentList $arguments -Wait -PassThru
        if ($process.ExitCode -ne 0) {
            Write-Output "WSL setup stopped with exit code $($process.ExitCode)."
            Complete-Setup 11
        }
    }
    catch {
        Write-Output "Windows did not approve WSL setup: $($_.Exception.Message)"
        Complete-Setup 11
    }

    Write-Output "WSL installation was requested. Restart Windows if it asks, open Ubuntu once"
    Write-Output "to finish creating your Linux account, then press Set up Ichiran again."
    Complete-Setup 12
}

function Offer-DockerInstall {
    Write-Output "Docker is not available inside WSL."
    Write-Output ""
    Write-Output "Ichiran's supported setup uses Docker Desktop to keep its PostgreSQL database"
    Write-Output "contained and recoverable. Docker Desktop is a separate application and may"
    Write-Output "show its own terms and administrator prompt."

    if (-not ($Install -and $Guided)) {
        Write-Output ""
        Write-Output "Install Docker Desktop, enable WSL integration, and run this again:"
        Write-Output "  https://www.docker.com/products/docker-desktop/"
        Complete-Setup 20
    }

    $winget = Get-Command winget.exe -ErrorAction SilentlyContinue
    if (-not $winget) {
        Write-Output "Windows Package Manager is unavailable, so Docker Desktop cannot be offered"
        Write-Output "automatically. Opening its download page instead."
        Start-Process 'https://www.docker.com/products/docker-desktop/'
        Complete-Setup 20
    }

    $answer = Read-Host "Install Docker Desktop with Windows Package Manager now? [y/N]"
    if ($answer -notmatch '(?i)^y(?:es)?$') { Complete-Setup 20 }

    & winget.exe install --id Docker.DockerDesktop --exact --interactive `
        --accept-source-agreements --accept-package-agreements
    if ($LASTEXITCODE -ne 0) {
        Write-Output "Docker Desktop setup stopped with exit code $LASTEXITCODE."
        Complete-Setup 21
    }

    Write-Output "Docker Desktop is installed. Open it once, finish its welcome screen, and make"
    Write-Output "sure Settings > Resources > WSL Integration includes your Ubuntu distribution."
    Write-Output "Then press Set up Ichiran again."
    Complete-Setup 22
}

function Start-DockerDesktop {
    $dockerDesktop = Join-Path $env:ProgramFiles 'Docker\Docker\Docker Desktop.exe'
    if (-not (Test-Path -LiteralPath $dockerDesktop)) { return }

    Write-Output "Docker Desktop is installed but not answering. Starting it..."
    Start-Process -FilePath $dockerDesktop
    for ($attempt = 0; $attempt -lt 45; $attempt++) {
        Start-Sleep -Seconds 2
        if (Test-Wsl 'docker info >/dev/null 2>&1') {
            Write-Output "Docker Desktop: ready"
            return
        }
    }
}

function Install-DockerWrapper([string] $LinuxHome) {
    # The wrapper is deliberately per-user and tiny. It adds bounded self-healing to the
    # upstream command: if Docker Desktop or the compose service was stopped between reads,
    # start the existing containers and retry for up to one minute. It never rebuilds and
    # never deletes the database.
    $wrapper = @'
#!/usr/bin/env bash
set -u
cd "$HOME/ichiran" || exit 1

last=""
for attempt in $(seq 1 30); do
  if output=$(docker compose exec -T main ichiran-cli "$@" 2>&1); then
    printf '%s' "$output"
    exit 0
  fi
  last="$output"
  if [ "$attempt" -eq 1 ]; then
    docker compose up -d >/dev/null 2>&1 || true
  fi
  sleep 2
done

printf '%s\n' "$last" >&2
exit 1
'@
    $bytes = [Text.Encoding]::UTF8.GetBytes($wrapper.Replace("`r`n", "`n"))
    $encoded = [Convert]::ToBase64String($bytes)
    $install = "mkdir -p ~/.local/bin && printf '%s' '$encoded' | base64 -d > ~/.local/bin/sakatsu-ichiran && chmod 700 ~/.local/bin/sakatsu-ichiran"
    $null = Invoke-Wsl $install
    if ($script:WslExitCode -ne 0) {
        throw 'could not install the per-user Ichiran launcher inside WSL'
    }

    $program = "$LinuxHome/.local/bin/sakatsu-ichiran"
    Save-AnalyzerCommand $program
    return $program
}

Write-Output "Sakatsu Reader - local Ichiran setup"
Write-Output "=====================================`n"
Write-Output "The built-in analyzer remains available throughout this setup."
Write-Output "Ichiran needs roughly 4.7 GB and can take a while on its first run.`n"

# WSL can exist as a Windows component without a usable distribution. Both states need
# the same next step, and neither should fall through into a page of bash errors.
$wsl = Get-Command wsl.exe -ErrorAction SilentlyContinue
if (-not $wsl) { Offer-WslInstall }
& wsl.exe --status *> $null
if ($LASTEXITCODE -ne 0) { Offer-WslInstall }
$distros = @(& wsl.exe --list --quiet 2>$null) | ForEach-Object { $_.Trim() } | Where-Object { $_ }
if ($distros.Count -eq 0) { Offer-WslInstall }

Write-Output "WSL: ready"
if ($Distribution) { Write-Output "Distribution: $Distribution" }

$linuxHome = ((Invoke-Wsl 'printf %s "$HOME"') -join '').Trim()
if (-not $linuxHome.StartsWith('/')) {
    Write-Output "Could not determine the Linux home directory: $linuxHome"
    Complete-Setup 13
}

# Preserve a native installation. It is faster than Docker and the app's default command
# already knows how to reach it; only a non-default distribution needs explicit routing.
$native = ((Invoke-Wsl 'command -v ichiran-cli || true') -join '').Trim()
if ($native) {
    Write-Output "ichiran-cli: $native"
    $probe = (Invoke-Wsl "ichiran-cli -f -- '$probeText' 2>&1 | head -c 240") -join ''
    if ($probe -match '^\s*\[') {
        Write-Output "Database: answering"
        if ($Distribution -and $Install) { Save-AnalyzerCommand $native }
        Write-Output "`nIchiran is ready. Choose Use Ichiran now in Sakatsu Reader."
        Complete-Setup 0
    }

    Write-Output "Database: not answering"
    if ($Install) {
        Write-Output "Trying to start PostgreSQL without an interactive administrator prompt..."
        $null = Invoke-Wsl 'sudo -n service postgresql start >/dev/null 2>&1 || true'
        $probe = (Invoke-Wsl "ichiran-cli -f -- '$probeText' 2>&1 | head -c 240") -join ''
        if ($probe -match '^\s*\[') {
            Write-Output "Database: answering after restart"
            if ($Distribution) { Save-AnalyzerCommand $native }
            Write-Output "`nIchiran is ready. Choose Use Ichiran now in Sakatsu Reader."
            Complete-Setup 0
        }
    }

    Write-Output "A native Ichiran installation exists but its database still cannot answer:"
    ($probe -split "`n" | Select-Object -First 4) | ForEach-Object { Write-Output "  $_" }
    Write-Output "Start PostgreSQL inside WSL, then run this check again:"
    Write-Output "  sudo service postgresql start"
    Complete-Setup 30
}

$docker = ((Invoke-Wsl 'command -v docker || true') -join '').Trim()
if (-not $docker) { Offer-DockerInstall }

if (-not (Test-Wsl 'docker info >/dev/null 2>&1') -and $Install) { Start-DockerDesktop }
if (-not (Test-Wsl 'docker info >/dev/null 2>&1')) {
    Write-Output "Docker is installed but is not available to this WSL distribution."
    Write-Output "Open Docker Desktop > Settings > Resources > WSL Integration, enable it for"
    Write-Output "this distribution, choose Apply & restart, then press Set up Ichiran again."
    Complete-Setup 23
}
Write-Output "Docker: ready"

$repoExists = Test-Wsl 'test -d ~/ichiran/.git'
if (-not $repoExists -and -not $Install) {
    Write-Output "Ichiran: not installed"
    Write-Output "Re-run with -Install to clone and build upstream's Docker setup."
    Complete-Setup 40
}

if (-not $repoExists) {
    Write-Output "`n1 of 3 - Downloading Ichiran's source..."
    if ($Distribution) {
        & wsl.exe -d $Distribution -e bash -lc 'git clone --depth 1 https://github.com/tshatrov/ichiran ~/ichiran'
    }
    else {
        & wsl.exe -e bash -lc 'git clone --depth 1 https://github.com/tshatrov/ichiran ~/ichiran'
    }
    if ($LASTEXITCODE -ne 0) { Complete-Setup 41 }
}
else {
    Write-Output "Ichiran source: already present (left unchanged)"
}

if (-not $Install) {
    $probe = (Invoke-Wsl "cd ~/ichiran && docker compose exec -T main ichiran-cli -f -- '$probeText' 2>&1 | head -c 240") -join ''
    if ($probe -match '^\s*\[') {
        Write-Output "Database: answering"
        Write-Output "Run again with -Install to configure Sakatsu Reader's per-user launcher."
        Complete-Setup 0
    }
    Write-Output "Database: not answering. Re-run with -Install to start or finish setup."
    Complete-Setup 40
}

if ($Install) {
    Write-Output "`n2 of 3 - Building Ichiran and PostgreSQL..."
    Write-Output "This is the longest step. Build output will continue below."
    if ($Distribution) {
        & wsl.exe -d $Distribution -e bash -lc 'cd ~/ichiran && docker compose build'
    }
    else {
        & wsl.exe -e bash -lc 'cd ~/ichiran && docker compose build'
    }
    if ($LASTEXITCODE -ne 0) { Complete-Setup 42 }

    Write-Output "`n3 of 3 - Starting the database and restoring its data..."
    if ($Distribution) {
        & wsl.exe -d $Distribution -e bash -lc 'cd ~/ichiran && docker compose up -d'
    }
    else {
        & wsl.exe -e bash -lc 'cd ~/ichiran && docker compose up -d'
    }
    if ($LASTEXITCODE -ne 0) { Complete-Setup 43 }
}

$launcher = Install-DockerWrapper $linuxHome
Write-Output "Waiting for Ichiran's database to answer..."
$probe = (Invoke-Wsl "$launcher -f -- '$probeText' 2>&1 | head -c 240") -join ''
if ($probe -notmatch '^\s*\[') {
    Write-Output "Ichiran did not become ready within one minute:"
    ($probe -split "`n" | Select-Object -First 5) | ForEach-Object { Write-Output "  $_" }
    Write-Output "The containers and database were left intact. Run setup again to continue;"
    Write-Output "it will reuse everything already downloaded and built."
    Complete-Setup 44
}

Write-Output "Database: answering"
Write-Output "`nIchiran is ready. Choose Use Ichiran now in Sakatsu Reader."
Complete-Setup 0
