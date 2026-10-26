<#
.SYNOPSIS
    Install, launch, optionally upgrade, and uninstall a Sakatsu NSIS installer in isolation.

.DESCRIPTION
    **Run it somewhere disposable.** An NSIS install registers with Windows (the per-user
    Uninstall key, Start menu shortcuts), and that registration is keyed by product name: a test
    install on a machine where Sakatsu is installed would take over the reader's own registration,
    and the test uninstall would then remove it. So the harness:

    - refuses while any copy of Sakatsu is running (single-instance forwarding would hand the
      launch to that copy), and never stops it;
    - refuses while Windows lists any Sakatsu installation;
    - installs only with -DisposableEnvironment, which says the machine may be changed: Windows
      Sandbox (see `run-release-qa-sandbox.ps1`) or a CI runner.

    Everything a run creates lives in one new folder inside -QaRoot, inside TEMP: the install
    folder and an isolated profile (APPDATA, LOCALAPPDATA, the device-data folder and the WebView2
    data folder). Only the process the run starts, and its descendants, are ever stopped. A passing
    run removes its folder; a failing one leaves it as evidence and says where.

    A launch must expose the real web interface through Windows accessibility, then remain alive
    for -LaunchSeconds. This checks rendering, not every interaction.
    -SandboxWebView2Runtime explicitly uses an installed runtime folder for sandbox QA only.

    With -PreviousInstaller this is the *installer-compatibility* test: install the previous
    version, seed settings, a word status and a resource, install the current version over it,
    and check that all three, and the app identity, survived. It does not test the in-app updater.

.EXAMPLE
    pwsh -File scripts/test-release-installer.ps1 -Installer setup.exe -ValidateOnly
#>
#requires -Version 5.1
[CmdletBinding()]
param(
    [Parameter(Mandatory)] [string] $Installer,
    [string] $PreviousInstaller,
    [switch] $PublicRelease,
    [switch] $DisposableEnvironment,
    [switch] $ValidateOnly,
    [string] $QaRoot = (Join-Path $env:TEMP 'SakatsuReleaseQA'),
    [int] $LaunchSeconds = 8,
    [int] $LaunchTimeoutSeconds = 60,
    [switch] $SandboxWebView2Runtime,
    [string] $ResultFile,
    # For the checks file: where to look for installed copies of Sakatsu.
    [string[]] $UninstallKeys
)

$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'release-qa.psm1') -Force

$result = [ordered]@{
    installer_sha256   = $null
    registration       = $null
    installer          = $null
    previous_installer = $null
    mode               = if ($PreviousInstaller) { 'upgrade' } else { 'install' }
    passed             = $false
    steps              = New-Object System.Collections.Generic.List[string]
    observations       = New-Object System.Collections.Generic.List[string]
    evidence           = $null
    error              = $null
}

function Step([string]$Text) {
    $result.steps.Add($Text)
    Write-Output "  $Text"
}

function Write-Result {
    if ($ResultFile) {
        Write-Utf8NoBom $ResultFile ($result | ConvertTo-Json -Depth 5)
    }
}

function Assert-Signed([string]$Path, [string]$Label) {
    $signature = Get-AuthenticodeSignature -LiteralPath $Path
    if ($signature.Status -ne 'Valid') {
        throw "$Label is not Authenticode-valid: $($signature.Status) $($signature.StatusMessage)"
    }
    if (-not $signature.TimeStamperCertificate) {
        throw "$Label has no trusted timestamp. A release must remain verifiable after certificate expiry."
    }
}

function Get-InstallerVersion([string]$Path) {
    $version = (Get-Item -LiteralPath $Path).VersionInfo.ProductVersion
    if ([string]::IsNullOrWhiteSpace($version)) { throw "The installer carries no product version: $Path" }
    return $version.Trim()
}

function Install-Into([string]$Setup, [string]$Folder) {
    # /D must come last and unquoted: that is NSIS's rule, not a style choice.
    $install = Start-Process -FilePath $Setup -ArgumentList '/S', "/D=$Folder" -Wait -PassThru -WindowStyle Hidden
    if ($install.ExitCode -ne 0) { throw "NSIS installation exited with $($install.ExitCode): $Setup" }
}

function Get-OurRegistration([string]$InstallDir, [hashtable]$KeyArguments) {
    $all = @(Get-RegisteredSakatsu @KeyArguments)
    $mine = @($all | Where-Object {
            $_.InstallLocation -and (Get-NormalizedPath $_.InstallLocation).Equals((Get-NormalizedPath $InstallDir), [System.StringComparison]::OrdinalIgnoreCase)
        })
    if ($all.Count -ne $mine.Count) {
        throw "Windows lists a Sakatsu installation this run did not create."
    }
    if ($mine.Count -ne 1) { throw "Expected one registration for this run's install; found $($mine.Count)." }
    return $mine[0]
}

function Assert-InstalledContents([string]$InstallDir, [switch]$Current) {
    $executable = Join-Path $InstallDir 'sakatsu-desktop.exe'
    if (-not (Test-Path -LiteralPath $executable)) { throw "The installed executable is missing: $executable" }
    foreach ($runtime in 'MSVCP140.dll', 'VCRUNTIME140.dll', 'VCRUNTIME140_1.dll') {
        if (-not (Test-Path -LiteralPath (Join-Path $InstallDir $runtime))) {
            throw "Installed runtime dependency is missing: $runtime"
        }
    }
    if ($Current) {
        foreach ($name in @("checkpoint-fixture.exe", "resolve-resources.exe", "verify-release.exe")) {
            if (Test-Path -LiteralPath (Join-Path $InstallDir $name)) { throw "Developer executable shipped: $name" }
        }
        # The licence, the notices and the guided setup are what this version must ship.
        foreach ($required in 'LICENSE', 'THIRD-PARTY-NOTICES.txt', 'scripts\setup-ichiran.ps1') {
            if (-not (Test-Path -LiteralPath (Join-Path $InstallDir $required))) {
                throw "The installer did not ship $required"
            }
        }
        if ($PublicRelease) { Assert-Signed $executable 'Installed executable' }
    }
    return $executable
}

<#
    The saved font size, from either shape of settings.json: the flat object versions before
    the settings file was versioned wrote, or `{ "version": 1, "settings": { ... } }`. A version
    that finds the old shape rewrites it in the new one and keeps the original as settings.json.v0.
#>
function Get-SavedFontSize([string]$Path) {
    $parsed = [System.IO.File]::ReadAllText($Path) | ConvertFrom-Json
    $nested = $parsed.PSObject.Properties['settings']
    if ($nested -and $parsed.PSObject.Properties['version']) { $parsed = $nested.Value }
    $size = $parsed.PSObject.Properties['font_size']
    if ($size) { return [double]$size.Value }
    return $null
}

function Get-SeedState([string]$Store, [string]$Profile) {
    $state = [ordered]@{ profile_exists = (Test-Path -LiteralPath $Profile) }
    foreach ($name in 'settings.json', 'words.json') {
        $path = Join-Path $Store $name
        if (Test-Path -LiteralPath $path) {
            $item = Get-Item -LiteralPath $path -Force
            $keys = ''
            if ($name -eq 'settings.json') {
                try {
                    $parsed = [System.IO.File]::ReadAllText($path) | ConvertFrom-Json
                    $keys = 'top-level keys: {0}; font_size={1}' -f (@($parsed.PSObject.Properties | ForEach-Object { $_.Name }) -join ','), (Get-SavedFontSize $path)
                } catch { $keys = 'unparseable' }
            }
            $state[$name] = '{0} bytes{1} {2}' -f $item.Length, $(if ($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) { ' (link)' } else { '' }), $keys
        } else {
            $state[$name] = 'missing'
        }
    }
    $state['store_entries'] = @(Get-ChildItem -LiteralPath $Store -Force -ErrorAction SilentlyContinue | ForEach-Object { $_.Name }) -join ', '
    return ($state.GetEnumerator() | ForEach-Object { "$($_.Key)=$($_.Value)" }) -join '; '
}

function Invoke-LaunchSmoke([string]$Executable, $Environment, [string]$RunDir) {
    $runtime = ''
    if ($SandboxWebView2Runtime) {
        $runtime = Get-QaWebView2Runtime
        $result.observations.Add("Sandbox-only runtime override: $runtime")
    }
    $owned = Start-IsolatedProcess $Executable $Environment $RunDir -WebView2Runtime $runtime
    try {
        $snapshot = @(Wait-InstalledInterface $owned -TimeoutSeconds $LaunchTimeoutSeconds)
        $result.observations.Add('Rendered web interface verified by accessibility: ' + (($snapshot | Where-Object { $_.Type -in @('ControlType.Document', 'ControlType.Button') } | ForEach-Object { "$($_.Type): $($_.Name)" }) -join '; '))
        Start-Sleep -Seconds $LaunchSeconds
        if ($owned.Process.HasExited) {
            throw "Launch exited early with $($owned.Process.ExitCode)"
        }
    } finally {
        [void](Stop-OwnedProcessTree $owned)
        $owned.Process.WaitForExit(10000) | Out-Null
    }
    $webview = $Environment['WEBVIEW2_USER_DATA_FOLDER']
    if (@(Get-ChildItem -LiteralPath $webview -Force -ErrorAction SilentlyContinue).Count -gt 0) {
        $result.observations.Add('WebView2 data was written to the isolated folder.')
    } else {
        $result.observations.Add('WebView2 wrote nothing to the isolated folder during the launch; its data location is unverified.')
    }
}

$runDir = $null
try {
    $installerPath = (Resolve-Path -LiteralPath $Installer).Path
    $result.installer = [System.IO.Path]::GetFileName($installerPath)
    $previousPath = $null
    if ($PreviousInstaller) {
        $previousPath = (Resolve-Path -LiteralPath $PreviousInstaller).Path
        $result.previous_installer = [System.IO.Path]::GetFileName($previousPath)
    }
    $keys = @{}
    if ($UninstallKeys) { $keys = @{ UninstallKeys = $UninstallKeys } }

    # ---- Preflight: every refusal comes before anything is created or changed. ----
    $root = Resolve-QaRoot $QaRoot

    $running = @(Get-SakatsuProcess)
    if ($running.Count -gt 0) {
        $ids = ($running | ForEach-Object { "$($_.ProcessName) (pid $($_.Id))" }) -join ', '
        throw "Sakatsu is already running: $ids. Close it first; this harness never stops a process it did not start."
    }

    $registered = @(Get-RegisteredSakatsu @keys)
    if ($registered.Count -gt 0) {
        $names = ($registered | ForEach-Object { "$($_.DisplayName) $($_.DisplayVersion)" }) -join ', '
        throw "Windows lists an installed Sakatsu ($names). A test install would take over that registration and its uninstall would remove it. Run this in Windows Sandbox with run-release-qa-sandbox.ps1."
    }

    if ($PublicRelease) {
        Assert-Signed $installerPath 'Installer'
    } else {
        $result.observations.Add("Development installer signature: $((Get-AuthenticodeSignature -LiteralPath $installerPath).Status)")
    }

    if ($ValidateOnly) {
        Write-Output 'Preflight passed. Nothing was installed.'
        $result.passed = $true
        return
    }
    if (-not $DisposableEnvironment) {
        throw 'Installing changes this machine: it registers with Windows and adds Start menu shortcuts. Pass -DisposableEnvironment only on a machine that is thrown away afterwards, or use run-release-qa-sandbox.ps1.'
    }

    # ---- The run's own folder. ----
    New-Item -ItemType Directory -Force -Path $root | Out-Null
    $name = 'run-{0}-{1}' -f (Get-Date -Format 'yyyyMMdd-HHmmss'), ([guid]::NewGuid().ToString('N').Substring(0, 8))
    $runDir = Assert-Contained (Join-Path $root $name) $root 'The run folder'
    if (Test-Path -LiteralPath $runDir) { throw "The run folder already exists: $runDir" }
    New-Item -ItemType Directory -Path $runDir | Out-Null
    $installDir = Join-Path $runDir 'install'
    $environment = New-IsolatedEnvironment $runDir
    Assert-IsolatedEnvironment $environment $runDir
    Write-Output "Run folder: $runDir"

    $currentVersion = Get-InstallerVersion $installerPath

    if ($previousPath) {
        $previousVersion = Get-InstallerVersion $previousPath
        Install-Into $previousPath $installDir
        $registration = Get-OurRegistration $installDir $keys
        Step "Installed the previous version, $($registration.DisplayVersion)."
        $executable = Assert-InstalledContents $installDir
        Invoke-LaunchSmoke $executable $environment $runDir
        Step 'The previous version rendered its web interface and stayed running.'

        # Seed what an upgrade must keep. The word is written as JSON escapes so this file
        # stays ASCII: U+6C34 U+307F U+305A is the word for water, read mizu.
        $store = Join-Path $environment['APPDATA'] 'com.sakatsu.app\store'
        Write-Utf8NoBom (Join-Path $store 'settings.json') '{ "font_size": 19.0 }'
        Write-Utf8NoBom (Join-Path $store 'words.json') '[ { "term": "\u6c34", "reading": "\u307f\u305a", "status": "known" } ]'
        $marker = Join-Path $environment['SAKATSU_DEVICE_DATA_DIR'] 'release-qa\marker.bin'
        New-Item -ItemType Directory -Force -Path ([System.IO.Path]::GetDirectoryName($marker)) | Out-Null
        $bytes = New-Object byte[] 4096
        (New-Object System.Random).NextBytes($bytes)
        [System.IO.File]::WriteAllBytes($marker, $bytes)
        $markerHash = (Get-FileHash -LiteralPath $marker -Algorithm SHA256).Hash
        Step 'Seeded settings, a word status and a resource marker.'
        $profileDir = Join-Path $environment['APPDATA'] 'com.sakatsu.app'
        $result.observations.Add("After seeding: $(Get-SeedState $store $profileDir)")

        Install-Into $installerPath $installDir
        $registration = Get-OurRegistration $installDir $keys
        if ($registration.DisplayVersion -ne $currentVersion) {
            throw "After the upgrade Windows lists $($registration.DisplayVersion), not $currentVersion."
        }
        Step "Upgraded $previousVersion to $currentVersion over the same folder; one registration."
        $result.observations.Add("After the upgrade install: $(Get-SeedState $store $profileDir)")
    } else {
        Install-Into $installerPath $installDir
        $registration = Get-OurRegistration $installDir $keys
        if ($registration.DisplayVersion -ne $currentVersion) {
            throw "Windows lists $($registration.DisplayVersion), not $currentVersion."
        }
        Step "Installed $currentVersion; one registration."
    }

    $result.installer_sha256 = (Get-FileHash -LiteralPath $installerPath -Algorithm SHA256).Hash
    $result.registration = [ordered]@{ DisplayName = $registration.DisplayName; DisplayVersion = $registration.DisplayVersion; scope = if ($registration.Key -match "HKEY_CURRENT_USER|HKCU") { "user" } else { "machine" } }
    $executable = Assert-InstalledContents $installDir -Current
    Step 'The executable, runtime DLLs, licence, notices and setup script are installed.'
    Invoke-LaunchSmoke $executable $environment $runDir
    Step 'The current version rendered its web interface and stayed running.'

    if ($previousPath) {
        $result.observations.Add("After the current version's launch: $(Get-SeedState $store $profileDir)")
        $fontSize = Get-SavedFontSize (Join-Path $store 'settings.json')
        if ($fontSize -ne 19.0) { throw "The upgrade lost a setting: font_size is $fontSize." }
        $words = @([System.IO.File]::ReadAllText((Join-Path $store 'words.json')) | ConvertFrom-Json)
        $kept = @($words | Where-Object { $_.term -eq ([string][char]0x6C34) -and $_.reading -eq ([string][char]0x307F + [char]0x305A) -and $_.status -eq 'known' })
        if ($kept.Count -ne 1) { throw 'The upgrade lost a word status.' }
        if ((Get-FileHash -LiteralPath $marker -Algorithm SHA256).Hash -ne $markerHash) { throw 'The upgrade changed an installed resource.' }
        if (-not (Test-Path -LiteralPath (Join-Path $environment['APPDATA'] 'com.sakatsu.app'))) { throw 'The app identity folder is gone.' }
        Step 'Settings, the word status, the resource and the identity folder survived the upgrade and a launch.'
    }

    # ---- Uninstall, through the registration this run created. ----
    $uninstaller = Join-Path $installDir 'uninstall.exe'
    [void](Assert-Contained $uninstaller $runDir 'The uninstaller')
    if (-not (Test-Path -LiteralPath $uninstaller)) { throw 'The uninstaller is missing.' }
    $uninstall = Start-Process -FilePath $uninstaller -ArgumentList '/S' -Wait -PassThru -WindowStyle Hidden
    if ($uninstall.ExitCode -ne 0) { throw "Uninstall exited with $($uninstall.ExitCode)" }
    # The NSIS uninstaller copies itself and returns before the copy finishes.
    $deadline = (Get-Date).AddSeconds(60)
    while ((Test-Path -LiteralPath $executable) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 500 }
    if (Test-Path -LiteralPath $executable) { throw 'The uninstaller left the executable behind.' }
    if (@(Get-RegisteredSakatsu @keys).Count -ne 0) { throw 'The uninstaller left the Windows registration behind.' }
    Step 'Uninstalled: the executable and the registration are gone.'

    $result.passed = $true
    Remove-OwnedFolder $runDir $root
    Write-Output "Installer QA passed: $([System.IO.Path]::GetFileName($installerPath))"
} catch {
    $result.error = $_.Exception.Message
    if ($runDir -and (Test-Path -LiteralPath $runDir)) {
        $result.evidence = $runDir
        # In a sandbox the folder goes when the sandbox closes, so the result carries a listing.
        $result.listing = @(Get-ChildItem -LiteralPath $runDir -Recurse -Force -Depth 4 -ErrorAction SilentlyContinue |
                Where-Object { $_.FullName -notlike '*\install\scripts\*' } |
                ForEach-Object {
                    $kind = if ($_.Attributes -band [System.IO.FileAttributes]::ReparsePoint) { '<link>' } elseif ($_.PSIsContainer) { '<dir>' } else { $_.Length }
                    '{0}  {1}' -f $_.FullName.Substring($runDir.Length), $kind
                })
        Write-Output "Run folder kept as evidence: $runDir"
    }
    throw
} finally {
    Write-Result
}
