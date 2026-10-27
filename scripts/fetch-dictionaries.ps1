<#
.SYNOPSIS
    Downloads the runtime dictionaries into the user profile.

.DESCRIPTION
    屠つ ships by fetch, not by bundle (project notes §6): these files are ~50MB, carry
    their own licences, and are never committed to this repository.

    Nothing here runs automatically. It is a deliberate, user-initiated download —
    run it once, or again when you want newer data.

    Both sources are CC BY-SA 4.0. That licence requires attribution, which is why
    the importer captures each dictionary's own `attribution` field from its
    index.json and keeps it beside the data (project notes §8). Do not strip it.

    URLs are the ones the projects themselves publish. Jitendex hosts its downloads
    on jitendex.org rather than attaching them to its GitHub releases, so its URL
    points at the release repo that page links to.

.PARAMETER Destination
    Where to put the zips. Defaults to the app data directory Tauri uses for
    identifier `com.sakatsu.app` on Windows.

.PARAMETER Force
    Re-download even if the file is already there.

.EXAMPLE
    pwsh -File scripts/fetch-dictionaries.ps1
#>
[CmdletBinding()]
param(
    [string] $Destination = (Join-Path $env:APPDATA 'com.sakatsu.app\dictionaries'),
    [switch] $Force
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$dictionaries = @(
    @{
        Name        = 'Jitendex'
        File        = 'jitendex-yomitan.zip'
        Url         = 'https://github.com/stephenmk/stephenmk.github.io/releases/download/2026.10.03.0/jitendex-yomitan.zip'
        Licence     = 'CC BY-SA 4.0 — Jitendex, built on JMdict (EDRDG)'
        Description = 'JMdict-derived, with example sentences and structured content'
    },
    @{
        Name        = 'JMnedict'
        File        = 'JMnedict.zip'
        Url         = 'https://github.com/yomidevs/jmdict-yomitan/releases/download/2026-10-18/JMnedict.zip'
        Licence     = 'CC BY-SA 4.0 — EDRDG'
        Description = 'names: people, places, companies — what JMdict deliberately omits'
    }
)

if (-not (Test-Path $Destination)) {
    New-Item -ItemType Directory -Force -Path $Destination | Out-Null
}
Write-Output "Destination: $Destination`n"

foreach ($dictionary in $dictionaries) {
    $path = Join-Path $Destination $dictionary.File

    if ((Test-Path $path) -and -not $Force) {
        $size = [math]::Round((Get-Item $path).Length / 1MB, 1)
        Write-Output "$($dictionary.Name): already present (${size} MB) — pass -Force to replace it"
        continue
    }

    Write-Output "$($dictionary.Name): downloading — $($dictionary.Description)"
    # Download beside the target and move into place, so an interrupted run cannot
    # leave a truncated zip that then fails to import for no obvious reason.
    $partial = "$path.partial"
    try {
        Invoke-WebRequest -Uri $dictionary.Url -OutFile $partial -MaximumRedirection 5
        Move-Item -Path $partial -Destination $path -Force
        $size = [math]::Round((Get-Item $path).Length / 1MB, 1)
        Write-Output "$($dictionary.Name): done (${size} MB)"
    }
    catch {
        if (Test-Path $partial) { Remove-Item $partial -Force }
        Write-Output "$($dictionary.Name): FAILED — $($_.Exception.Message)"
        Write-Output "  If the URL has moved, check the project's own downloads page rather than guessing."
        continue
    }
}

Write-Output "`nLicences — these travel with the data and must be attributed:"
foreach ($dictionary in $dictionaries) {
    Write-Output "  $($dictionary.Name): $($dictionary.Licence)"
}
Write-Output "`n屠つ imports each zip on first use into a sharded index beside it."
