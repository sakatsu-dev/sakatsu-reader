<#
.SYNOPSIS
    Containment, ownership and isolation helpers for the release installer harness.

.DESCRIPTION
    Kept apart from `test-release-installer.ps1` so `test-release-installer.Tests.ps1` can check
    each rule on its own, without building or running an installer.

    Three rules, each of which the previous harness broke:

    - **Containment is by path component.** `C:\Temp2\qa` is not inside `C:\Temp`, though the
      string starts with it. Every component between the base and the path must also be an
      ordinary folder: a junction or symlink there would take a delete outside the base.
    - **Only what this run started is stopped.** A process is identified by its id *and* its
      start time, and its descendants are found by parent id. Nothing is ever stopped by name:
      the reader's own copy of Sakatsu may be running beside a test.
    - **The profile is the run's own.** The app reads `APPDATA`, `LOCALAPPDATA` and
      `SAKATSU_DEVICE_DATA_DIR`, and WebView2 honours `WEBVIEW2_USER_DATA_FOLDER`; all four must
      point inside the run folder before anything is launched.

    ASCII only, and compatible with Windows PowerShell 5.1: that is what Windows Sandbox has.
#>
#requires -Version 5.1
Set-StrictMode -Version 3.0

$script:IgnoreCase = [System.StringComparison]::OrdinalIgnoreCase

# The app's process name, and the name it had before the rename. The second is written as
# character codes so this file stays ASCII.
$script:ProcessNames = @('sakatsu-desktop', ([string][char]0x5C60 + [char]0x3064))

# Where Windows lists installed programs. Overridable so a test can point at a key it owns.
$script:DefaultUninstallKeys = @(
    'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall',
    'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall',
    'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall'
)

if (-not ('ReleaseQa.NativePath' -as [type])) {
    Add-Type -Namespace ReleaseQa -Name NativePath -MemberDefinition @'
[DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
public static extern uint GetLongPathName(string shortPath, System.Text.StringBuilder longPath, uint bufferLength);
'@
}

<# The long form of an existing path: `FETHFI~1` and the name it abbreviates are one folder. #>
function Get-LongPath([string]$Existing) {
    $buffer = New-Object System.Text.StringBuilder 1024
    $length = [ReleaseQa.NativePath]::GetLongPathName($Existing, $buffer, [uint32]$buffer.Capacity)
    if ($length -eq 0 -or $length -ge $buffer.Capacity) { return $Existing }
    return $buffer.ToString()
}

<#
    Absolute, without a trailing separator, and with 8.3 short names expanded.

    `GetFullPath` resolves `..` but leaves `FETHFI~1` as it is, and TEMP is often written that way,
    so the same folder could arrive in two spellings. The deepest part of the path that exists is
    expanded; the rest, which does not exist yet, cannot contain a short name.
#>
function Get-NormalizedPath([string]$Path) {
    if ([string]::IsNullOrWhiteSpace($Path)) { throw 'An empty path was given.' }
    $full = [System.IO.Path]::GetFullPath($Path)
    $root = [System.IO.Path]::GetPathRoot($full)
    if ($full.Length -gt $root.Length) { $full = $full.TrimEnd('\', '/') }
    $existing = $full
    $rest = ''
    while (-not (Test-Path -LiteralPath $existing)) {
        $parent = [System.IO.Path]::GetDirectoryName($existing)
        if (-not $parent) { return $full }
        $rest = [System.IO.Path]::GetFileName($existing) + $(if ($rest) { '\' + $rest } else { '' })
        $existing = $parent
    }
    $long = (Get-LongPath $existing)
    if ($long.Length -gt [System.IO.Path]::GetPathRoot($long).Length) { $long = $long.TrimEnd('\') }
    if ($rest) { return (Join-Path $long $rest) }
    return $long
}

<# True when $Path is strictly inside $Parent, component by component. Never true for $Parent itself. #>
function Test-PathWithin([string]$Path, [string]$Parent) {
    $child = Get-NormalizedPath $Path
    $base = Get-NormalizedPath $Parent
    if ($child.Equals($base, $script:IgnoreCase)) { return $false }
    return $child.StartsWith($base.TrimEnd('\') + '\', $script:IgnoreCase)
}

<# Refuse a reparse point (junction, symlink) at $Path or any component between it and $Base. $Base itself is not checked. #>
function Assert-NoReparsePoint([string]$Path, [string]$Base) {
    $current = Get-NormalizedPath $Path
    $stop = Get-NormalizedPath $Base
    while (-not $current.Equals($stop, $script:IgnoreCase)) {
        if (Test-Path -LiteralPath $current) {
            $item = Get-Item -LiteralPath $current -Force
            if ($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) {
                throw "Refusing a path through a junction or symbolic link: $current"
            }
        }
        $parent = [System.IO.Path]::GetDirectoryName($current)
        if (-not $parent -or $parent.Equals($current, $script:IgnoreCase)) {
            throw "$Path is not inside $Base"
        }
        $current = $parent
    }
}

<# The path, if it is strictly inside $Base through ordinary folders only; otherwise throw. #>
function Assert-Contained([string]$Path, [string]$Base, [string]$What = 'Path') {
    if (-not (Test-PathWithin $Path $Base)) {
        throw "$What must be inside $(Get-NormalizedPath $Base), not the folder itself or beside it: $(Get-NormalizedPath $Path)"
    }
    Assert-NoReparsePoint $Path $Base
    return (Get-NormalizedPath $Path)
}

<# The QA root: a folder strictly inside TEMP. #>
function Resolve-QaRoot([string]$QaRoot, [string]$Temp = $env:TEMP) {
    return (Assert-Contained $QaRoot $Temp 'The QA root')
}

<# Running copies of Sakatsu, by either name. Read-only: nothing here stops them. #>
function Get-SakatsuProcess {
    return @(Get-Process -Name $script:ProcessNames -ErrorAction SilentlyContinue)
}

<# Installed copies of Sakatsu that Windows knows about. #>
function Get-RegisteredSakatsu([string[]]$UninstallKeys = $script:DefaultUninstallKeys) {
    $found = @()
    foreach ($key in $UninstallKeys) {
        if (-not (Test-Path -LiteralPath $key)) { continue }
        foreach ($entry in Get-ChildItem -LiteralPath $key -ErrorAction SilentlyContinue) {
            $properties = Get-ItemProperty -LiteralPath $entry.PSPath -ErrorAction SilentlyContinue
            if (-not $properties) { continue }
            $name = $properties.PSObject.Properties['DisplayName']
            if ($name -and [string]$name.Value -like 'Sakatsu Reader*') {
                $location = $properties.PSObject.Properties['InstallLocation']
                $version = $properties.PSObject.Properties['DisplayVersion']
                $uninstall = $properties.PSObject.Properties['UninstallString']
                $found += [pscustomobject]@{
                    Key             = $entry.PSPath
                    DisplayName     = [string]$name.Value
                    DisplayVersion  = if ($version) { [string]$version.Value } else { '' }
                    InstallLocation = if ($location) { ([string]$location.Value).Trim('"') } else { '' }
                    UninstallString = if ($uninstall) { [string]$uninstall.Value } else { '' }
                }
            }
        }
    }
    return $found
}

<# The four profile variables for a run, each inside its own folder of $RunDir. #>
function New-IsolatedEnvironment([string]$RunDir) {
    return [ordered]@{
        APPDATA                    = Join-Path $RunDir 'Roaming'
        LOCALAPPDATA               = Join-Path $RunDir 'Local'
        SAKATSU_DEVICE_DATA_DIR    = Join-Path $RunDir 'device'
        WEBVIEW2_USER_DATA_FOLDER  = Join-Path $RunDir 'webview'
    }
}

<# Throw unless every profile variable is set and inside $RunDir. #>
function Assert-IsolatedEnvironment($Environment, [string]$RunDir) {
    foreach ($name in 'APPDATA', 'LOCALAPPDATA', 'SAKATSU_DEVICE_DATA_DIR', 'WEBVIEW2_USER_DATA_FOLDER') {
        if (-not $Environment.Contains($name) -or [string]::IsNullOrWhiteSpace($Environment[$name])) {
            throw "The isolated profile does not set $name."
        }
        [void](Assert-Contained $Environment[$name] $RunDir $name)
    }
}

<# Start $FilePath with the isolated profile, and return the process and its start time. #>
function Start-IsolatedProcess([string]$FilePath, $Environment, [string]$RunDir, [string]$Arguments = '', [string]$WebView2Runtime = '') {
    Assert-IsolatedEnvironment $Environment $RunDir
    $info = New-Object System.Diagnostics.ProcessStartInfo
    $info.FileName = $FilePath
    $info.Arguments = $Arguments
    $info.UseShellExecute = $false
    # A console program gets a hidden console of its own rather than writing into the caller's.
    $info.CreateNoWindow = $true
    $info.WorkingDirectory = [System.IO.Path]::GetDirectoryName($FilePath)
    foreach ($name in $Environment.Keys) {
        New-Item -ItemType Directory -Force -Path $Environment[$name] | Out-Null
        $info.EnvironmentVariables[$name] = $Environment[$name]
    }
    # This workaround is explicit test setup, never an inherited developer override.
    $info.EnvironmentVariables.Remove('WEBVIEW2_BROWSER_EXECUTABLE_FOLDER')
    if ($WebView2Runtime) {
        if (-not [System.IO.Path]::IsPathRooted($WebView2Runtime) -or
            -not (Test-Path -LiteralPath (Join-Path $WebView2Runtime 'msedgewebview2.exe') -PathType Leaf)) {
            throw 'The requested WebView2 runtime has no executable.'
        }
        $info.EnvironmentVariables['WEBVIEW2_BROWSER_EXECUTABLE_FOLDER'] = $WebView2Runtime
    }
    # Developer overrides would make the run test something other than the shipped defaults.
    foreach ($name in 'SAKATSU_ANALYZER', 'SAKATSU_ANALYZER_FAULT', 'SAKATSU_ICHIRAN_CMD') {
        if ($info.EnvironmentVariables.ContainsKey($name)) { $info.EnvironmentVariables.Remove($name) }
    }
    $process = [System.Diagnostics.Process]::Start($info)
    return [pscustomobject]@{ Process = $process; Id = $process.Id; StartTime = $process.StartTime }
}

<# A process or an error dialog is not a rendered app. Require web content and an app control. #>
function Test-InstalledInterface($Snapshot) {
    $documents = @($Snapshot | Where-Object { $_.Type -eq 'ControlType.Document' -and -not $_.Offscreen })
    $controls = @($Snapshot | Where-Object {
        $_.Type -eq 'ControlType.Button' -and -not $_.Offscreen -and
        $_.Name -in @('Read', 'Settings', 'Set up Sakatsu Reader', 'Start reading')
    })
    return $documents.Count -gt 0 -and $controls.Count -gt 0
}

function Get-InstalledInterfaceSnapshot([int]$ProcessId) {
    Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
    $ae = [System.Windows.Automation.AutomationElement]
    $condition = New-Object System.Windows.Automation.PropertyCondition($ae::ProcessIdProperty, $ProcessId)
    $windows = $ae::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children, $condition)
    foreach ($window in $windows) {
        # The window is owned by the launched PID; WebView descendants may use renderer PIDs.
        $elements = $window.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
        foreach ($element in $elements) {
            $current = $element.Current
            [pscustomobject]@{ Type = $current.ControlType.ProgrammaticName; Name = $current.Name; Offscreen = $current.IsOffscreen }
        }
    }
}

function Wait-InstalledInterface($Owned, [int]$TimeoutSeconds = 60, [scriptblock]$ReadSnapshot) {
    if (-not $ReadSnapshot) { $ReadSnapshot = { Get-InstalledInterfaceSnapshot $Owned.Id } }
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        if ($Owned.Process.HasExited) { throw "Launch exited before rendering the app (exit $($Owned.Process.ExitCode))." }
        $snapshot = @(& $ReadSnapshot)
        if (@($snapshot | Where-Object { $_.Name -match 'Could not find the WebView2 Runtime' }).Count) {
            throw 'The launch showed a WebView2 runtime error instead of the app.'
        }
        if (Test-InstalledInterface $snapshot) { return $snapshot }
        if ((Get-Date) -ge $deadline) { break }
        Start-Sleep -Milliseconds 250
    } while ($true)
    $names = ($snapshot | ForEach-Object { "$($_.Type): $($_.Name)" }) -join '; '
    throw "The process stayed alive but no Sakatsu web interface appeared within $TimeoutSeconds seconds. Last accessibility snapshot: $names"
}

<# Windows Sandbox may register a runtime version whose files are missing. No downloads or registry edits. #>
function Get-QaWebView2Runtime {
    $folders = foreach ($root in @("${env:ProgramFiles(x86)}\Microsoft\EdgeWebView\Application", "$env:LOCALAPPDATA\Microsoft\EdgeWebView\Application")) {
        Get-ChildItem -LiteralPath $root -Directory -ErrorAction SilentlyContinue | Where-Object {
            $_.Name -match '^\d+\.\d+\.\d+\.\d+$' -and
            (Test-Path -LiteralPath (Join-Path $_.FullName 'msedgewebview2.exe') -PathType Leaf)
        }
    }
    $runtime = $folders | Sort-Object { [version]$_.Name } -Descending | Select-Object -First 1
    if (-not $runtime) { throw 'No on-disk WebView2 runtime is available for the explicit sandbox workaround.' }
    return $runtime.FullName
}

<# Stop the process this run started and its descendants, and nothing else. #>
function Stop-OwnedProcessTree($Owned) {
    if (-not $Owned) { return @() }
    $all = @(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue)
    $ids = New-Object System.Collections.Generic.List[int]
    $root = Get-Process -Id $Owned.Id -ErrorAction SilentlyContinue
    # Same id, same start time: otherwise the id has been reused by somebody else's process.
    if ($root -and $root.StartTime -eq $Owned.StartTime) { $ids.Add($Owned.Id) }
    $frontier = @($Owned.Id)
    while ($frontier.Count -gt 0) {
        $next = @()
        foreach ($parentId in $frontier) {
            foreach ($child in $all | Where-Object { $_.ParentProcessId -eq $parentId }) {
                # Created after the owned process started, or it is an older process whose
                # parent id merely collides.
                if ($child.CreationDate -ge $Owned.StartTime -and -not $ids.Contains([int]$child.ProcessId)) {
                    $ids.Add([int]$child.ProcessId)
                    $next += [int]$child.ProcessId
                }
            }
        }
        $frontier = $next
    }
    $stopped = @()
    # Children first, so none is orphaned and re-parented mid-walk.
    for ($i = $ids.Count - 1; $i -ge 0; $i--) {
        $target = Get-Process -Id $ids[$i] -ErrorAction SilentlyContinue
        if ($target) {
            Stop-Process -Id $ids[$i] -Force -ErrorAction SilentlyContinue
            $stopped += $ids[$i]
        }
    }
    return $stopped
}

<# Delete a tree without following links: a junction or symlink inside it is removed as a link, and what it points at is never touched. #>
function Remove-TreeWithoutFollowing([string]$Directory) {
    foreach ($entry in [System.IO.Directory]::GetFileSystemEntries($Directory)) {
        $attributes = [System.IO.File]::GetAttributes($entry)
        $isDirectory = ($attributes -band [System.IO.FileAttributes]::Directory) -ne 0
        if ($attributes -band [System.IO.FileAttributes]::ReparsePoint) {
            # Non-recursive delete of a link removes the link alone.
            if ($isDirectory) { [System.IO.Directory]::Delete($entry, $false) } else { [System.IO.File]::Delete($entry) }
        } elseif ($isDirectory) {
            Remove-TreeWithoutFollowing $entry
        } else {
            [System.IO.File]::SetAttributes($entry, [System.IO.FileAttributes]::Normal)
            [System.IO.File]::Delete($entry)
        }
    }
    [System.IO.Directory]::Delete($Directory, $false)
}

<#
    Remove a folder only if it is contained in $Base through ordinary folders.

    Not `Remove-Item -Recurse`: in Windows PowerShell 5.1 that can follow a junction inside the
    folder and delete the contents of whatever it points at.
#>
function Remove-OwnedFolder([string]$Path, [string]$Base) {
    $contained = Assert-Contained $Path $Base 'A folder to remove'
    if (Test-Path -LiteralPath $contained) {
        Remove-TreeWithoutFollowing $contained
    }
}

<# Write UTF-8 without a byte-order mark: serde_json refuses one, and PowerShell 5.1 adds it by default. #>
function Write-Utf8NoBom([string]$Path, [string]$Text) {
    New-Item -ItemType Directory -Force -Path ([System.IO.Path]::GetDirectoryName($Path)) | Out-Null
    [System.IO.File]::WriteAllText($Path, $Text, (New-Object System.Text.UTF8Encoding $false))
}

Export-ModuleMember -Function Get-NormalizedPath, Test-PathWithin, Assert-NoReparsePoint,
    Assert-Contained, Resolve-QaRoot, Get-SakatsuProcess, Get-RegisteredSakatsu,
    New-IsolatedEnvironment, Assert-IsolatedEnvironment, Start-IsolatedProcess,
    Stop-OwnedProcessTree, Remove-OwnedFolder, Write-Utf8NoBom, Test-InstalledInterface,
    Get-InstalledInterfaceSnapshot, Wait-InstalledInterface, Get-QaWebView2Runtime
