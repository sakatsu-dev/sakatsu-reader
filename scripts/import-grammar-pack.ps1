<#
.SYNOPSIS
    Rebuilds core/src/grammar/packs/hanabira.json from its CC0 source.

.DESCRIPTION
    project notes §5 says grammar packs are rebuilt from the original CC/public sources
    rather than copied from anyone's site. This is that rebuild.

    Source: https://github.com/tristcoil/hanabira.org-japanese-content
    Licence: CC0-1.0 — a public-domain dedication, so no attribution is legally
    required. It is credited in the pack's own `description` anyway, because saying
    where data came from is the point of §8 and costs nothing.

    (Imabi and Tae Kim remain off limits for this: both are all-rights-reserved and may
    only be linked, never imported.)

    The output is committed. This script only needs running when the upstream content
    changes — and the diff is then reviewable, which is why the pack is generated into
    the repository rather than fetched at runtime.

    These entries carry **no match rules**, so nothing here is auto-detected in text.
    They are a reference shelf: several hundred JLPT grammar points to look up. The
    curated `core.json` is what the engine actually matches on.

.PARAMETER Output
    Where to write the pack.

.EXAMPLE
    pwsh -File scripts/import-grammar-pack.ps1
#>
[CmdletBinding()]
param(
    [string] $Output = (Join-Path $PSScriptRoot '..\core\src\grammar\packs\hanabira.json')
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$base = 'https://raw.githubusercontent.com/tristcoil/hanabira.org-japanese-content/main/grammar_json'
$levels = @('N5', 'N4', 'N3', 'N2', 'N1')

# Ids must be stable across runs and unique across the pack: they are what the UI and
# any future cross-reference key off.
$seen = @{}
function New-Id([string] $title, [string] $level) {
    # The romaji in parentheses makes a far better slug than the Japanese does.
    $romaji = if ($title -match '\(([^)]+)\)') { $Matches[1] } else { $title }
    $slug = ($romaji.ToLower() -replace '[^a-z0-9]+', '-').Trim('-')
    if (-not $slug) { $slug = 'point' }
    $id = "hanabira-$($level.ToLower())-$slug"

    # Two grammar points can slugify the same; a suffix keeps both.
    if ($seen.ContainsKey($id)) {
        $seen[$id] += 1
        $id = "$id-$($seen[$id])"
    }
    else {
        $seen[$id] = 1
    }
    return $id
}

$patterns = [System.Collections.Generic.List[object]]::new()

foreach ($level in $levels) {
    $url = "$base/grammar_ja_${level}_full_alphabetical_0001.json"
    Write-Output "fetching $level …"
    $entries = (Invoke-WebRequest -Uri $url -MaximumRedirection 5).Content | ConvertFrom-Json

    foreach ($entry in $entries) {
        $title = [string] $entry.title
        if (-not $title) { continue }

        # "A が いちばん～ (A ga ichiban～)" → the Japanese half is the pattern.
        $written = ($title -replace '\s*\([^)]*\)\s*$', '').Trim()
        if (-not $written) { $written = $title }

        $examples = @()
        if ($entry.PSObject.Properties.Name -contains 'examples' -and $entry.examples) {
            $examples = @($entry.examples | Where-Object { $_.jp } | Select-Object -First 4 | ForEach-Object {
                    [ordered]@{ japanese = [string] $_.jp; english = [string] $_.en }
                })
        }

        $pattern = [ordered]@{
            id          = New-Id $title $level
            pattern     = $written
            title       = $title
            jlpt        = $level
            explanation = [string] $entry.short_explanation
        }
        if ($entry.formation) { $pattern.formation = [string] $entry.formation }
        if ($entry.long_explanation) { $pattern.detail = [string] $entry.long_explanation }
        if ($examples.Count -gt 0) { $pattern.examples = $examples }

        $patterns.Add($pattern)
    }
}

$pack = [ordered]@{
    name        = 'hanabira'
    description = 'JLPT grammar reference, rebuilt from tristcoil/hanabira.org-japanese-content (CC0-1.0). Reference only: these entries carry no match rules, so nothing here is detected automatically — core.json is what the engine matches on.'
    patterns    = $patterns
}

$json = $pack | ConvertTo-Json -Depth 8
# ConvertTo-Json escapes non-ASCII as \uXXXX. Valid JSON, but a pack nobody can read in
# a diff is a pack nobody can review, so the escapes are turned back into characters.
$json = [System.Text.RegularExpressions.Regex]::Replace(
    $json, '\\u([0-9a-fA-F]{4})', { param($m) [char][Convert]::ToInt32($m.Groups[1].Value, 16) })

$full = [System.IO.Path]::GetFullPath($Output)
[System.IO.File]::WriteAllText($full, $json + "`n", (New-Object System.Text.UTF8Encoding $false))

$size = [math]::Round((Get-Item $full).Length / 1KB, 1)
Write-Output "`nwrote $($patterns.Count) grammar points to $full (${size} KB)"
