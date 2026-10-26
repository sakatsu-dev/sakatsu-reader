<#
.SYNOPSIS
    Takes a verifiable snapshot of the source tree, and optionally the built installers.

.DESCRIPTION
    Reproduces the convention already in the snapshot folder, which had been carried out by
    hand each time and reconstructed from the previous run's `_INFO.txt`. What that convention
    is, and why:

      - **`.git` travels with it.** The snapshot is a working repository, not a pile of files:
        `git -C <snapshot> log` answers, which is what makes it useful a year later. This is the
        main difference from `clipboard-ocr-service/scripts/make_snapshot.ps1`, which belongs to
        a different project and excludes `.git`. Do not follow that one for this repository.

      - **The manifest lives beside the snapshot, not inside it.** A `SHA256SUMS` file within
        the tree is a file that has to exempt itself from its own hashing, and it changes what
        it is describing. Sidecar `_SHA256.txt` and `_INFO.txt` have neither problem.

      - **Generated and locally ignored directories are excluded.** Git supplies local-only
        exclusions so machine-specific directory names stay out of this script.
        Fetched models and dictionaries live outside the tree anyway and are reproduced by
        `scripts\fetch-*.ps1`.

    Nothing is deleted and nothing in the working tree is touched. `-IncludeBuild` additionally
    copies the Tauri installers into a sibling `Sakatsu_Build_<stamp>` directory, because the
    source snapshot deliberately has no `target`.

.PARAMETER Destination
    Where snapshots are kept. Defaults to `$env:SAKATSU_SNAPSHOT_DIR`, or `target\snapshots` in the
    repository when that is unset — never a path on somebody's own drive.

.PARAMETER IncludeBuild
    Also archive `target\release\bundle`'s installers. **Build them first if they matter**: this
    copies whatever is there, and reports how old it is against the newest source file so a
    stale bundle cannot be filed as though it were current.

.PARAMETER Verify
    Re-hash every copied file against the manifest before reporting success. Slower, and worth
    it: a snapshot nobody checked is a backup nobody has tested.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File .\scripts\make-snapshot.ps1 -Verify

.EXAMPLE
    cargo tauri build
    powershell -ExecutionPolicy Bypass -File .\scripts\make-snapshot.ps1 -IncludeBuild -Verify
#>
[CmdletBinding()]
param(
    [string]$Destination = $(if ($env:SAKATSU_SNAPSHOT_DIR) { $env:SAKATSU_SNAPSHOT_DIR }
                             else { Join-Path $PSScriptRoot '..\target\snapshots' }),
    [switch]$IncludeBuild,
    [switch]$Verify
)

$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
if (-not (Test-Path (Join-Path $root 'src-tauri\tauri.conf.json'))) {
    throw "This does not look like the 屠つ repository: $root"
}
if (-not (Test-Path $Destination)) {
    throw "Destination does not exist: $Destination. Create it, or pass -Destination."
}

$stamp = Get-Date -Format 'yyyyMMdd_HHmmss'
$name = "Sakatsu_Snapshot_$stamp"
$dst = Join-Path $Destination $name
if (Test-Path $dst) { throw "Snapshot already exists: $dst" }

# Relative, so they are readable in the INFO file, and resolved for robocopy which wants paths.
$excludeRel = @(
    'target', 'ui\node_modules', 'ui\dist', '.installer-qa',
    'src-tauri\gen', 'src-tauri\windows-runtime'
)
# Use Git's local and shared rules instead of embedding machine-specific exclusions.
$ignored = @(& git -C $root -c core.quotepath=false ls-files --others --ignored --exclude-standard --directory)
if ($LASTEXITCODE -ne 0) { throw 'Could not read Git exclusions; snapshot not created.' }
$excludeRel += @($ignored | Where-Object { $_.EndsWith('/') } | ForEach-Object { $_.TrimEnd('/') })
$excludeRel = @($excludeRel | Sort-Object -Unique)

Write-Host "Snapshotting -> $dst"
New-Item -ItemType Directory -Path $dst -Force | Out-Null

$roboArgs = @($root, $dst, '/E', '/XD') + ($excludeRel | ForEach-Object { Join-Path $root $_ }) +
            @('/NFL', '/NDL', '/NJH', '/NJS', '/NP', '/R:1', '/W:1')
$null = & robocopy @roboArgs
# robocopy: 0-7 report what it did, 8 and above are failures.
if ($LASTEXITCODE -ge 8) { throw "robocopy failed with exit code $LASTEXITCODE" }

function Write-Manifest {
    param([string]$Directory, [string]$ManifestPath)

    $files = Get-ChildItem $Directory -Recurse -File -Force
    Write-Host "Hashing $($files.Count) files..."

    $lines = New-Object System.Collections.Generic.List[string]
    $lines.Add('# SHA-256, byte length, relative path')
    $files |
        Sort-Object { $_.FullName.Substring($Directory.Length + 1).Replace('\', '/') } |
        ForEach-Object {
            $relative = $_.FullName.Substring($Directory.Length + 1).Replace('\', '/')
            $hash = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLower()
            $lines.Add("$hash`t$($_.Length)`t$relative")
        }
    Set-Content -LiteralPath $ManifestPath -Value $lines -Encoding utf8

    [pscustomobject]@{
        Count = $files.Count
        Bytes = ($files | Measure-Object -Property Length -Sum).Sum
        Hash  = (Get-FileHash -LiteralPath $ManifestPath -Algorithm SHA256).Hash.ToLower()
    }
}

function Test-Manifest {
    param([string]$Directory, [string]$ManifestPath)

    $expected = @{}
    Get-Content -LiteralPath $ManifestPath | Select-Object -Skip 1 | ForEach-Object {
        $parts = $_ -split "`t"
        $expected[$parts[2]] = $parts[0]
    }

    $bad = 0
    Get-ChildItem $Directory -Recurse -File -Force | ForEach-Object {
        $relative = $_.FullName.Substring($Directory.Length + 1).Replace('\', '/')
        $hash = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLower()
        if ($expected[$relative] -ne $hash) {
            $bad++
            Write-Warning "MISMATCH: $relative"
        }
    }
    if ($bad -gt 0) { throw "$bad file(s) do not match the manifest" }
    Write-Host "Verified $($expected.Count) files against the manifest."
}

$manifest = Join-Path $Destination "${name}_SHA256.txt"
$source = Write-Manifest -Directory $dst -ManifestPath $manifest

Push-Location $root
try {
    $branch = (git rev-parse --abbrev-ref HEAD).Trim()
    $head = (git rev-parse HEAD).Trim()
    $status = (git status --short) -join "`n"
} finally {
    Pop-Location
}

# The newest source file, so build-vs-source currency can be stated rather than assumed.
$newest = Get-ChildItem $root -Recurse -File -ErrorAction SilentlyContinue |
    Where-Object {
        $sourcePath = $_.FullName
        $sourcePath -notlike '*\.git\*' -and -not @($excludeRel | Where-Object {
            $sourcePath.StartsWith((Join-Path $root $_) + '\', [System.StringComparison]::OrdinalIgnoreCase)
        }).Count
    } |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1

@"
Snapshot: $name
Created: $(Get-Date -Format o)
Source: $root
Destination: $dst
Git branch: $branch
Git HEAD: $head
Files: $($source.Count)
Bytes: $($source.Bytes)
SHA-256 manifest: $manifest
Manifest SHA-256: $($source.Hash)
Verified: $(if ($Verify) { 'yes, every file re-hashed' } else { 'no, run with -Verify' })
Newest source file: $($newest.LastWriteTime.ToString('s'))  $($newest.Name)
Excluded reproducible/generated directories: $($excludeRel -join ', ')
Working tree status follows:
$status
"@ | Set-Content -LiteralPath (Join-Path $Destination "${name}_INFO.txt") -Encoding utf8

if ($Verify) { Test-Manifest -Directory $dst -ManifestPath $manifest }
Write-Host ("Source snapshot complete: {0} files, {1:N1} MB" -f $source.Count, ($source.Bytes / 1MB))

if (-not $IncludeBuild) {
    Write-Host "`n  $dst"
    return
}

# --- The built installers, as a sibling ------------------------------------------------
#
# Beside the snapshot rather than inside it: the source snapshot excludes `target` on purpose,
# and burying a ~150 MB installer in a 45 MB source tree would quietly change what a snapshot is.

$buildName = "Sakatsu_Build_$stamp"
$buildDst = Join-Path $Destination $buildName
$bundle = Join-Path $root 'target\release\bundle'
# NSIS only since the WebView2 runtime was embedded - see README. The MSI target was retired
# rather than shipped at ~170 MB beside an installer that does the same job.
$artifacts = @(
    (Join-Path $bundle 'nsis\Sakatsu Reader_0.1.0_x64-setup.exe')
)
$missing = $artifacts | Where-Object { -not (Test-Path -LiteralPath $_) }
if ($missing) {
    throw "No installers to archive. Run ``cargo tauri build`` first. Missing:`n  $($missing -join "`n  ")"
}

New-Item -ItemType Directory -Path $buildDst -Force | Out-Null
$artifacts | ForEach-Object { Copy-Item -LiteralPath $_ -Destination $buildDst }

$buildManifest = Join-Path $Destination "${buildName}_SHA256.txt"
$build = Write-Manifest -Directory $buildDst -ManifestPath $buildManifest

$built = (Get-ChildItem $buildDst -File | Sort-Object LastWriteTime -Descending |
    Select-Object -First 1).LastWriteTime
$stale = $built -lt $newest.LastWriteTime

# The release binary is inside both installers, so it is recorded by hash rather than copied.
$exe = Join-Path $root 'target\release\sakatsu-desktop.exe'
$exeLine = if (Test-Path -LiteralPath $exe) {
    $item = Get-Item -LiteralPath $exe
    "  built $($item.LastWriteTime.ToString('s'))  $([math]::Round($item.Length/1MB,1)) MB`n" +
    "  SHA-256 $((Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash.ToLower())"
} else {
    '  not present'
}

@"
Build archive: $buildName
Created: $(Get-Date -Format o)
Source: $root
Destination: $buildDst
Companion source snapshot: $name
Git branch: $branch
Git HEAD: $head
Files: $($build.Count)
Bytes: $($build.Bytes)
SHA-256 manifest: $buildManifest
Manifest SHA-256: $($build.Hash)

Archived distributables (release profile, Tauri bundle):
$((Get-ChildItem $buildDst -File | ForEach-Object {
    "  {0}  {1:N1} MB  built {2:s}" -f $_.Name, ($_.Length/1MB), $_.LastWriteTime
}) -join "`n")

Release binary NOT copied (it is inside the installers above):
  target\release\sakatsu-desktop.exe
$exeLine

Build-vs-source currency:
  newest source file  $($newest.LastWriteTime.ToString('s'))  $($newest.Name)
  installers built    $($built.ToString('s'))
  $(if ($stale) {
      'STALE: the installers predate the newest source file, so they do NOT contain everything
  in the companion snapshot. Rebuild and archive again if this matters.'
  } else {
      'Current: the installers were built after the last source change.'
  })

Not included, by the same reasoning as the source snapshot: fetched OCR models and dictionaries
(reproduced by scripts\fetch-*.ps1), per-user application data, and the .installer-qa tree.
"@ | Set-Content -LiteralPath (Join-Path $Destination "${buildName}_INFO.txt") -Encoding utf8

if ($Verify) { Test-Manifest -Directory $buildDst -ManifestPath $buildManifest }
Write-Host ("Build archive complete: {0} files, {1:N1} MB" -f $build.Count, ($build.Bytes / 1MB))
if ($stale) {
    Write-Warning "The installers predate the newest source file. They do not contain everything in this snapshot."
}

Write-Host "`n  $dst`n  $buildDst"
