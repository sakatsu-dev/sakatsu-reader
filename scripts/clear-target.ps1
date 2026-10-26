<#
.SYNOPSIS
    Safely reclaim all Cargo build storage for this workspace.

.DESCRIPTION
    `trim-target.ps1` preserves artifacts Cargo can still reach. This is the deliberate
    full reset for a target directory that has outgrown the drive: it shows the resolved
    path, file count and size, checks for executables still running from that directory,
    asks before deleting, delegates the removal to `cargo clean`, and verifies the result.

    Source, Git history, downloaded dictionaries, models and application data are outside
    Cargo's target directory and are not touched. The next Cargo build will be slower
    because every Rust dependency must be compiled again.

.PARAMETER Force
    Skip the confirmation prompt. Path and process safety checks still run.

.PARAMETER AllowCustomTarget
    Permit a Cargo target directory other than this repository's `target` folder, or a
    `target` folder implemented as a junction/symbolic link. The resolved path is always
    printed first. Broad paths such as a drive root or a parent of the repository are
    refused even with this switch.

.EXAMPLE
    pwsh -File scripts/clear-target.ps1

.EXAMPLE
    pwsh -File scripts/clear-target.ps1 -WhatIf

.EXAMPLE
    pwsh -File scripts/clear-target.ps1 -Force
#>
[CmdletBinding(SupportsShouldProcess = $true, ConfirmImpact = 'High')]
param(
    [switch]$Force,
    [switch]$AllowCustomTarget
)

$ErrorActionPreference = 'Stop'

function Format-ByteSize([long]$Bytes) {
    if ($Bytes -ge 1GB) { return '{0:N2} GiB' -f ($Bytes / 1GB) }
    if ($Bytes -ge 1MB) { return '{0:N2} MiB' -f ($Bytes / 1MB) }
    if ($Bytes -ge 1KB) { return '{0:N2} KiB' -f ($Bytes / 1KB) }
    return "$Bytes bytes"
}

function Get-DirectoryStats([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path -PathType Container)) {
        return [pscustomobject]@{ Files = 0; Bytes = [long]0 }
    }

    $measure = Get-ChildItem -LiteralPath $Path -Recurse -Force -File |
        Measure-Object -Property Length -Sum
    # `Measure-Object` reports a null Sum for an empty directory rather than zero. Written out
    # rather than with `??`, which is PowerShell 7 syntax: this parses in the Windows PowerShell
    # that ships with the operating system, and `??` does not parse there at all.
    $bytes = if ($null -eq $measure.Sum) { 0 } else { $measure.Sum }
    [pscustomobject]@{
        Files = $measure.Count
        Bytes = [long] $bytes
    }
}

function Get-DriveFreeBytes([string]$Path) {
    try {
        $root = [System.IO.Path]::GetPathRoot($Path)
        return [long]([System.IO.DriveInfo]::new($root).AvailableFreeSpace)
    } catch {
        # A mapped/UNC target can be cleaned safely even if Windows cannot report its quota.
        return $null
    }
}

$root = [System.IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
$manifest = Join-Path $root 'Cargo.toml'
$expectedTarget = [System.IO.Path]::GetFullPath((Join-Path $root 'target'))

if (-not (Test-Path -LiteralPath $manifest -PathType Leaf)) {
    throw "workspace manifest not found at $manifest"
}
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw 'cargo is not on PATH'
}

# Cargo configuration can redirect target-dir far outside the checkout. Asking Cargo is
# more accurate than assuming `target`, but deleting an unexpected answer is unsafe, so a
# custom location needs an explicit opt-in.
#
# **Cargo writes UTF-8; PowerShell decodes a child process's stdout using the console's
# output encoding.** Those agree on an English install and do not on this one, where the
# console codepage is 932 and the repository directory is named with two Japanese
# characters. The path came back as three replacement characters, so the equality check
# below refused the workspace's own target directory - and printed a path that looked
# character-for-character identical to the expected one, because by then it was being
# rendered rather than compared.
#
# It failed intermittently, which is worse than failing: a terminal already switched to
# UTF-8 worked, and the same command from another one did not.
Push-Location $root
$previousOutputEncoding = [Console]::OutputEncoding
try {
    # Guarded: a host with no real console can refuse the setter, and the read is still
    # worth attempting - it only mis-decodes a path that is not pure ASCII.
    try { [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false } catch { }
    $metadataText = & cargo metadata --manifest-path $manifest --no-deps --format-version 1
    if ($LASTEXITCODE -ne 0) { throw "cargo metadata failed with exit code $LASTEXITCODE" }
    $metadata = $metadataText | ConvertFrom-Json
} finally {
    try { [Console]::OutputEncoding = $previousOutputEncoding } catch { }
    Pop-Location
}

$target = [System.IO.Path]::GetFullPath([string]$metadata.target_directory)
$targetTrimmed = $target.TrimEnd([System.IO.Path]::DirectorySeparatorChar)
$rootTrimmed = $root.TrimEnd([System.IO.Path]::DirectorySeparatorChar)
$driveRoot = [System.IO.Path]::GetPathRoot($target).TrimEnd([System.IO.Path]::DirectorySeparatorChar)
$targetContainsRoot = $rootTrimmed.StartsWith(
    "$targetTrimmed$([System.IO.Path]::DirectorySeparatorChar)",
    [System.StringComparison]::OrdinalIgnoreCase
)

# These checks stay unconditional: neither a force flag nor custom-target opt-in should
# ever turn a workspace cleanup command into a repository or drive-root deletion.
if ($targetTrimmed -eq $rootTrimmed -or $targetContainsRoot -or $targetTrimmed -eq $driveRoot) {
    throw "refusing unsafe Cargo target directory: $target"
}

$isExpected = $target.Equals($expectedTarget, [System.StringComparison]::OrdinalIgnoreCase)
$targetItem = Get-Item -LiteralPath $target -Force -ErrorAction SilentlyContinue
$isReparsePoint = $targetItem -and
    (($targetItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0)

if ((-not $isExpected -or $isReparsePoint) -and -not $AllowCustomTarget) {
    $reason = if ($isReparsePoint) { 'is a junction or symbolic link' } else { 'is outside the expected workspace location' }
    throw "Cargo target directory $reason`: $target`nReview it, then re-run with -AllowCustomTarget if it is intentional."
}

Write-Host "Cargo target: $target" -ForegroundColor Cyan
if (-not (Test-Path -LiteralPath $target -PathType Container)) {
    Write-Host 'No target directory exists — nothing to clear.' -ForegroundColor Green
    exit 0
}

# A running Tauri app or test binary holds files in target. Detecting executables by their
# actual path avoids killing or blocking unrelated Cargo work elsewhere on the machine.
$targetPrefix = "$targetTrimmed$([System.IO.Path]::DirectorySeparatorChar)"
$rootPattern = [regex]::Escape($root)
$running = @(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | Where-Object {
    $executable = [string]$_.ExecutablePath
    $commandLine = [string]$_.CommandLine
    ($executable -and $executable.StartsWith($targetPrefix, [System.StringComparison]::OrdinalIgnoreCase)) -or
    ($_.Name -match '^(cargo|rustc)(\.exe)?$' -and $commandLine -match $rootPattern)
})

if ($running.Count -gt 0) {
    Write-Host 'These project processes are still using the build tree:' -ForegroundColor Yellow
    $running | Select-Object ProcessId, Name, ExecutablePath | Format-Table -AutoSize
    # **A preview still has to preview.** -WhatIf is documented as showing what would happen
    # and changing nothing, so a lock it would have run into is something to report, not a
    # reason to fail. Throwing here made the one safe way to inspect the situation unusable at
    # exactly the moment someone was using it to find out why the real run refused - the app
    # running app is the normal reason for both, and you could not look without
    # closing the thing you were trying to ask about.
    if (-not $WhatIfPreference) {
        throw 'Stop the desktop app/build/test and run the cleanup again.'
    }
    Write-Host 'A real run would stop here; -WhatIf continues so the preview is still useful.' -ForegroundColor Yellow
}

$before = Get-DirectoryStats $target
$freeBefore = Get-DriveFreeBytes $target
Write-Host ("Files:        {0:N0}" -f $before.Files)
Write-Host "Build cache:  $(Format-ByteSize $before.Bytes)"
if ($null -ne $freeBefore) { Write-Host "Drive free:   $(Format-ByteSize $freeBefore)" }
Write-Host 'The next Cargo build will recompile every Rust dependency.' -ForegroundColor Yellow

$action = "Delete all $(Format-ByteSize $before.Bytes) of Cargo build artifacts"
$approved = if ($Force -and -not $WhatIfPreference) {
    $true
} else {
    $PSCmdlet.ShouldProcess($target, $action)
}

if (-not $approved) {
    Write-Host 'Nothing deleted.' -ForegroundColor Yellow
    exit 0
}

Push-Location $root
try {
    & cargo clean --manifest-path $manifest
    $cleanExit = $LASTEXITCODE
} finally {
    Pop-Location
}
if ($cleanExit -ne 0) {
    throw "cargo clean failed with exit code $cleanExit"
}

$after = Get-DirectoryStats $target
if ($after.Files -ne 0 -or $after.Bytes -ne 0) {
    throw "cargo clean returned success but $($after.Files) file(s) remain in $target"
}

$freeAfter = Get-DriveFreeBytes $target
Write-Host ''
Write-Host "Cleared $(Format-ByteSize $before.Bytes) from Cargo's target directory." -ForegroundColor Green
if ($null -ne $freeBefore -and $null -ne $freeAfter) {
    # Cast both operands: PowerShell otherwise selects Math.Max(Int32, Int32), which
    # overflows precisely when this script has reclaimed more than about 2 GiB.
    $observed = [math]::Max([long]0, [long]($freeAfter - $freeBefore))
    Write-Host "Drive free: $(Format-ByteSize $freeAfter) ($(Format-ByteSize $observed) observed increase)"
}
Write-Host 'Source, Git data, downloaded models, dictionaries and app data were not touched.'
