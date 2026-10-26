[CmdletBinding()]
param(
    [Parameter(Mandatory)] [string] $SourceRoot,
    [string] $Output
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$catalogPath = Join-Path $PSScriptRoot 'offline-essentials-catalog.json'
$catalog = Get-Content -LiteralPath $catalogPath -Raw | ConvertFrom-Json
$workspace = Get-Content -LiteralPath (Join-Path $repo 'Cargo.toml') -Raw
$versionMatch = [regex]::Match($workspace, '(?m)^version\s*=\s*"([^"]+)"')
if (-not $versionMatch.Success) { throw 'Could not read the workspace version.' }
$appVersion = $versionMatch.Groups[1].Value

$source = [System.IO.Path]::GetFullPath($SourceRoot)
if (-not (Test-Path -LiteralPath $source -PathType Container)) {
    throw "SourceRoot is not a directory: $source"
}
if (-not $Output) {
    $Output = Join-Path $repo "Sakatsu-essentials-$appVersion.zip"
}
$outputPath = [System.IO.Path]::GetFullPath($Output)
if (Test-Path -LiteralPath $outputPath) {
    throw "Refusing to overwrite an existing package: $outputPath"
}

$stage = Join-Path ([System.IO.Path]::GetTempPath()) ("sakatsu-essentials-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $stage | Out-Null
try {
    $manifestComponents = @()
    foreach ($component in $catalog.components) {
        $componentStage = Join-Path $stage (Join-Path 'components' $component.id)
        New-Item -ItemType Directory -Path $componentStage -Force | Out-Null
        foreach ($artifact in $component.artifacts) {
            $inputPath = Join-Path $source (Join-Path $component.id $artifact.file)
            if (-not (Test-Path -LiteralPath $inputPath -PathType Leaf)) {
                throw "Required artifact is missing: $inputPath"
            }
            $file = Get-Item -LiteralPath $inputPath
            if ($file.Length -ne [long]$artifact.bytes) {
                throw "$($component.id)/$($artifact.file) has $($file.Length) bytes; expected $($artifact.bytes)."
            }
            $hash = (Get-FileHash -LiteralPath $inputPath -Algorithm SHA256).Hash.ToLowerInvariant()
            if ($hash -ne $artifact.sha256.ToLowerInvariant()) {
                throw "$($component.id)/$($artifact.file) failed SHA-256 verification."
            }
            Copy-Item -LiteralPath $inputPath -Destination (Join-Path $componentStage $artifact.file)
        }
        $manifestComponents += [ordered]@{
            id = $component.id
            artifacts = $component.artifacts
            licences = $component.licences
        }
    }

    $manifest = [ordered]@{
        package_version = $catalog.package_version
        compatible_app_versions = @($appVersion)
        components = $manifestComponents
    }
    $manifest | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $stage 'manifest.json') -Encoding utf8NoBOM

    Add-Type -AssemblyName System.IO.Compression.FileSystem
    [System.IO.Compression.ZipFile]::CreateFromDirectory(
        $stage,
        $outputPath,
        [System.IO.Compression.CompressionLevel]::Optimal,
        $false
    )
    $packageHash = (Get-FileHash -LiteralPath $outputPath -Algorithm SHA256).Hash.ToLowerInvariant()
    Set-Content -LiteralPath "$outputPath.sha256" -Value "$packageHash  $(Split-Path -Leaf $outputPath)" -Encoding ascii
    Write-Output "Built $outputPath"
    Write-Output "SHA-256 $packageHash"
} finally {
    $resolvedStage = [System.IO.Path]::GetFullPath($stage)
    $resolvedTemp = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
    if ($resolvedStage.StartsWith($resolvedTemp, [System.StringComparison]::OrdinalIgnoreCase)) {
        Remove-Item -LiteralPath $resolvedStage -Recurse -Force -ErrorAction SilentlyContinue
    }
}
