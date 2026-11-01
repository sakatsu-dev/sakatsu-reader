[CmdletBinding()]
param(
    [string] $SourceRoot,
    [string] $Output,
    [switch] $FetchMissing
)
$ErrorActionPreference = 'Stop'
$arguments = @((Join-Path $PSScriptRoot 'prepare-essentials.py'))
if ($SourceRoot) { $arguments += @('--source-root', $SourceRoot) }
if ($Output) { $arguments += @('--output', $Output) }
if ($FetchMissing) { $arguments += '--fetch-missing' }
& python @arguments
if ($LASTEXITCODE) { throw "Essentials preparation failed ($LASTEXITCODE)." }
