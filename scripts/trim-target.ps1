<#
.SYNOPSIS
    Delete build artifacts nothing can reach any more.

.DESCRIPTION
    Cargo never reclaims old artifacts. Every rebuild whose inputs hash differently leaves
    the previous one behind for good, and deleting a source file leaves its output behind
    for good as well. On this project that reached 64.5 GB, of which the overwhelming
    majority was unreachable: 211 debug-symbol files for about 26 binaries, 21 copies of
    sakatsu_core, five 328 MB copies of libort_sys, and a 95 MB `tokenprobe` from a test
    deleted the same afternoon.

    This removes only artifacts that nothing refers to. It is not a size-based clean and it
    does not touch debug information, optimisation, or anything that would change what a
    build produces or how long it takes.

    Three categories, because they do not cost the same:

      Orphaned    Output whose source no longer exists. Free — nothing will ever want it.
      Superseded  Older copies of an artifact that has a newer one. Cargo rebuilds
                  anything it turns out to still need, so the cost is at most one compile.
      Incremental The incremental-compilation cache, which was 23.4 GB by itself. Pure
                  cache, but it is what makes rebuilding changed code fast — so it is
                  behind -Incremental rather than part of a normal run.

.PARAMETER Confirm
    Actually delete. Without it this only reports, which is the default on purpose: a
    script that works out a directory name and then removes gigabytes from it should show
    its work first.

.PARAMETER Incremental
    Also clear the incremental cache. Costs a slower next rebuild.

.EXAMPLE
    pwsh -File scripts/trim-target.ps1
    pwsh -File scripts/trim-target.ps1 -Confirm
    pwsh -File scripts/trim-target.ps1 -Confirm -Incremental
#>
[CmdletBinding()]
param(
    [switch]$Confirm,
    [switch]$Incremental
)

$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$target = Join-Path $root 'target'
if (-not (Test-Path $target)) {
    Write-Host "no target directory at $target — nothing to do"
    exit 0
}

function Get-SizeGB($files) {
    if (-not $files) { return 0.0 }
    [math]::Round((($files | Measure-Object Length -Sum).Sum) / 1GB, 2)
}

$before = Get-SizeGB (Get-ChildItem $target -Recurse -File -Force -ErrorAction SilentlyContinue)
Write-Host "target is $before GB" -ForegroundColor Cyan
if (-not $Confirm) {
    Write-Host "(reporting only — pass -Confirm to delete)" -ForegroundColor Yellow
}
Write-Host ""

# --------------------------------------------------------------------------------------
# What sources exist right now.
#
# A crate or test artifact is named after its source: `tests/jlpt_coverage.rs` produces
# `jlpt_coverage-<hash>.exe`. Cargo normalises hyphens to underscores, so both spellings
# are collected — `sakatsu-core` on disk is `sakatsu_core` in an artifact name.
# --------------------------------------------------------------------------------------
$live = [System.Collections.Generic.HashSet[string]]::new()

foreach ($manifest in Get-ChildItem $root -Recurse -Filter Cargo.toml -File -ErrorAction SilentlyContinue) {
    if ($manifest.FullName.StartsWith($target)) { continue }
    foreach ($line in Get-Content $manifest.FullName) {
        if ($line -match '^\s*name\s*=\s*"([^"]+)"') {
            [void]$live.Add(($Matches[1] -replace '-', '_'))
        }
    }
    # Test, example and bench binaries are named after their files.
    $crateRoot = $manifest.Directory.FullName
    foreach ($dir in 'tests', 'examples', 'benches') {
        $path = Join-Path $crateRoot $dir
        if (Test-Path $path) {
            foreach ($file in Get-ChildItem $path -Filter *.rs -File) {
                [void]$live.Add(($file.BaseName -replace '-', '_'))
            }
        }
    }
}

# Build scripts and the crates behind them.
[void]$live.Add('build_script_build')
[void]$live.Add('build_script_main')

$workspaceNames = $live.Count

# Every dependency the build can still reach.
#
# Without this, the orphan rule condemns the entire dependency tree: the workspace's own
# manifests never mention `ort-sys`, so five 328 MB copies of it looked like output from a
# deleted source. They are not — they are five builds of a live dependency, which is the
# *superseded* rule's business, one copy to keep and four to drop.
#
# Getting that distinction wrong would not have corrupted anything, since Cargo rebuilds
# whatever it misses. It would have forced a full rebuild of every dependency and reported
# the reason as "source no longer exists", which is simply false.
$lock = Join-Path $root 'Cargo.lock'
if (Test-Path $lock) {
    foreach ($line in Get-Content $lock) {
        if ($line -match '^name\s*=\s*"([^"]+)"') {
            [void]$live.Add(($Matches[1] -replace '-', '_'))
        }
    }
}

Write-Host "$workspaceNames workspace crates and tests, $($live.Count) names live in total (incl. Cargo.lock)"

# --------------------------------------------------------------------------------------
# Every artifact, grouped by the family it belongs to.
#
# `libsakatsu_core-1a2b3c.rlib` and `sakatsu_core-1a2b3c.pdb` are two families of the same
# crate; a family is (directory, stem-without-hash, extension) so that keeping "the newest"
# keeps one of each kind rather than one in total.
# --------------------------------------------------------------------------------------
# Each profile directory explicitly, rather than a `*\deps` glob. That form silently
# matches nothing here — it reported 0 files where the explicit path finds 10,359 — and a
# cleanup script that quietly finds nothing looks exactly like a directory already clean.
$artifacts = @()
foreach ($profileDir in Get-ChildItem $target -Directory -ErrorAction SilentlyContinue) {
    foreach ($sub in 'deps', 'build') {
        $path = Join-Path $profileDir.FullName $sub
        if (Test-Path $path) {
            $artifacts += Get-ChildItem $path -Recurse -File -Force -ErrorAction SilentlyContinue
        }
    }
}

$orphaned = [System.Collections.Generic.List[object]]::new()
$families = @{}

foreach ($file in $artifacts) {
    # Strip Cargo's 16-hex-digit disambiguator to get the family stem.
    $stem = $file.BaseName -replace '-[0-9a-f]{16}$', ''
    $crate = $stem -replace '^lib', ''

    # A build-script directory is named `<crate>-<hash>`; its contents belong to the crate.
    if ($file.Directory.Name -match '^(.+)-[0-9a-f]{16}$') {
        $crate = $Matches[1] -replace '-', '_'
    }

    if (-not $live.Contains($crate) -and -not $live.Contains($stem)) {
        # Only judge things whose name should match a source. Anything else is left alone.
        if ($file.Extension -in '.exe', '.pdb', '.rlib', '.rmeta', '.d') {
            $orphaned.Add($file)
            continue
        }
    }

    $key = "$($file.DirectoryName)|$stem|$($file.Extension)"
    if (-not $families.ContainsKey($key)) {
        $families[$key] = [System.Collections.Generic.List[object]]::new()
    }
    $families[$key].Add($file)
}

# Newest of each family stays; the rest are superseded.
$superseded = [System.Collections.Generic.List[object]]::new()
foreach ($group in $families.Values) {
    if ($group.Count -le 1) { continue }
    $sorted = $group | Sort-Object LastWriteTime -Descending
    foreach ($old in $sorted | Select-Object -Skip 1) { $superseded.Add($old) }
}

Write-Host ""
Write-Host "orphaned   $($orphaned.Count) files, $(Get-SizeGB $orphaned) GB  (source no longer exists)"
if ($orphaned.Count -gt 0) {
    $orphaned | Sort-Object Length -Descending | Select-Object -First 5 |
        ForEach-Object { "             {0,6:N0} MB  {1}" -f ($_.Length / 1MB), $_.Name }
}
Write-Host "superseded $($superseded.Count) files, $(Get-SizeGB $superseded) GB  (a newer copy exists)"
if ($superseded.Count -gt 0) {
    $superseded | Sort-Object Length -Descending | Select-Object -First 5 |
        ForEach-Object { "             {0,6:N0} MB  {1}" -f ($_.Length / 1MB), $_.Name }
}

$incrementalDirs = @()
foreach ($profileDir in Get-ChildItem $target -Directory -ErrorAction SilentlyContinue) {
    $path = Join-Path $profileDir.FullName 'incremental'
    if (Test-Path $path) { $incrementalDirs += Get-Item $path }
}
$incrementalFiles = $incrementalDirs | ForEach-Object {
    Get-ChildItem $_.FullName -Recurse -File -Force -ErrorAction SilentlyContinue
}
$incrementalGB = Get-SizeGB $incrementalFiles
if ($Incremental) {
    Write-Host "incremental $($incrementalFiles.Count) files, $incrementalGB GB  (clearing — next rebuild will be slower)"
} elseif ($incrementalGB -gt 0) {
    Write-Host "incremental $incrementalGB GB  (kept; pass -Incremental to clear, at the cost of a slower next rebuild)" -ForegroundColor DarkGray
}

$doomed = @($orphaned) + @($superseded)
$reclaim = Get-SizeGB $doomed
if ($Incremental) { $reclaim += $incrementalGB }

Write-Host ""
Write-Host "would reclaim $reclaim GB of $before GB" -ForegroundColor Cyan

if (-not $Confirm) {
    Write-Host "nothing deleted. Re-run with -Confirm." -ForegroundColor Yellow
    exit 0
}

$failed = 0
foreach ($file in $doomed) {
    try { Remove-Item $file.FullName -Force -ErrorAction Stop }
    catch { $failed++ }   # Locked by a running build or antivirus; it will still be there next time.
}
if ($Incremental) {
    foreach ($dir in $incrementalDirs) {
        try { Remove-Item $dir.FullName -Recurse -Force -ErrorAction Stop } catch { $failed++ }
    }
}

$after = Get-SizeGB (Get-ChildItem $target -Recurse -File -Force -ErrorAction SilentlyContinue)
Write-Host ""
Write-Host "target is now $after GB (was $before GB)" -ForegroundColor Green
if ($failed -gt 0) {
    Write-Host "$failed file(s) were in use and left alone — close any running build and re-run" -ForegroundColor Yellow
}
