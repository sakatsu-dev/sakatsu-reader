<# Prepare a local WinGet singleton manifest from a signed installer and its successful QA report. #>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Installer,
    [Parameter(Mandatory)][string]$Evidence,
    [Parameter(Mandatory)][string]$PackageIdentifier,
    [Parameter(Mandatory)][string]$Publisher,
    [Parameter(Mandatory)][string]$RepositoryUrl,
    [Parameter(Mandatory)][string]$InstallerUrl,
    [Parameter(Mandatory)][string]$OutDir
)
$ErrorActionPreference = 'Stop'
if ($PackageIdentifier -notmatch '^[A-Za-z0-9][A-Za-z0-9.-]+\.[A-Za-z0-9][A-Za-z0-9.-]+$') { throw 'Invalid WinGet package identifier' }
foreach ($value in @($RepositoryUrl, $InstallerUrl)) {
    $uri = [Uri]$value
    if ($uri.Scheme -ne 'https' -or $uri.UserInfo -or $uri.Query -or $uri.Fragment -or $uri.IsLoopback) { throw 'Public HTTPS URLs without credentials are required' }
}
$installerPath = (Resolve-Path -LiteralPath $Installer).Path
$signature = Get-AuthenticodeSignature -LiteralPath $installerPath
if ($signature.Status -ne 'Valid' -or -not $signature.TimeStamperCertificate) { throw 'A timestamped Authenticode signature is required' }
$proof = Get-Content -Raw -LiteralPath $Evidence | ConvertFrom-Json
$hash = (Get-FileHash -LiteralPath $installerPath -Algorithm SHA256).Hash
if (-not $proof.passed -or $proof.installer_sha256 -ne $hash -or -not $proof.registration) { throw 'A successful QA report for these exact installer bytes is required' }
$registration = $proof.registration
if ($registration.scope -notin @('user','machine') -or -not $registration.DisplayName -or -not $registration.DisplayVersion) { throw 'The observed installation metadata is incomplete' }
$manifest = [ordered]@{
    PackageIdentifier = $PackageIdentifier
    PackageVersion = $registration.DisplayVersion
    PackageLocale = 'en-US'
    Publisher = $Publisher
    PackageName = $registration.DisplayName
    PackageUrl = $RepositoryUrl
    License = 'GPL-3.0-only'
    LicenseUrl = "$($RepositoryUrl.TrimEnd('/'))/blob/v$($registration.DisplayVersion)/LICENSE"
    ShortDescription = 'Capture Japanese text, look up words, and study while you read.'
    Installers = @([ordered]@{
        Architecture = 'x64'
        InstallerType = 'nullsoft'
        Scope = $registration.scope
        InstallerUrl = $InstallerUrl
        InstallerSha256 = $hash
        InstallerSwitches = @{ Silent = '/S'; SilentWithProgress = '/P'; Upgrade = '/S /UPDATE' }
        AppsAndFeaturesEntries = @(@{ DisplayName = $registration.DisplayName; DisplayVersion = $registration.DisplayVersion })
    })
    ManifestType = 'singleton'
    ManifestVersion = '1.6.0'
}
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$path = Join-Path $OutDir "$PackageIdentifier.yaml"
if (Test-Path -LiteralPath $path) { throw 'Refusing to overwrite an existing manifest' }
# JSON is a YAML subset; using the serializer avoids quoting or interpolation mistakes.
[IO.File]::WriteAllText($path, ($manifest | ConvertTo-Json -Depth 8), (New-Object Text.UTF8Encoding($false)))
Write-Output "Prepared $path. Validate with winget validate --manifest $OutDir before submission."
