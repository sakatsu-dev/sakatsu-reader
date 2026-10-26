<#
.SYNOPSIS
    Checks for the release installer harness's containment, ownership and refusal rules.

.DESCRIPTION
    Nothing here builds, installs or launches Sakatsu. The harness is only ever run with
    -ValidateOnly, or into a refusal, and every process these checks start is a copy of ping.exe
    that they stop by its own id.

    Each case is one rule the previous harness broke: it compared paths as strings, ran against
    the reader's own profile, and stopped every process with Sakatsu's name.

    What the checks create, they own: a folder in TEMP, a folder in the repository's ignored
    scratch/, and a registry key under HKCU\Software that stands in for Windows' list of
    installed programs. All three are removed at the end.

    Run:
        pwsh -File scripts/test-release-installer.Tests.ps1
        powershell -File scripts/test-release-installer.Tests.ps1   (Windows PowerShell 5.1)

    -Module points the direct checks at another copy of release-qa.psm1, so a deliberately broken
    copy can be shown to fail them.
#>
#requires -Version 5.1
[CmdletBinding()]
param(
    [string] $Module
)

$ErrorActionPreference = 'Stop'
# Resolved here, not as the parameter's default: Windows PowerShell 5.1 leaves $PSScriptRoot
# empty while defaults are evaluated.
if (-not $Module) { $Module = Join-Path $PSScriptRoot 'release-qa.psm1' }
Import-Module $Module -Force
$harness = Join-Path $PSScriptRoot 'test-release-installer.ps1'
$repo = Split-Path -Parent $PSScriptRoot
$failures = 0

function Test-Case([string]$Name, [scriptblock]$Body) {
    try {
        & $Body
        Write-Host "  ok    $Name" -ForegroundColor Green
    } catch {
        $script:failures++
        Write-Host "  FAIL  $Name" -ForegroundColor Red
        Write-Host "        $($_.Exception.Message)"
    }
}

function Assert-True($Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}

function Assert-Throws([scriptblock]$Body, [string]$Pattern) {
    try {
        & $Body
    } catch {
        if ($_.Exception.Message -notmatch $Pattern) {
            throw "It refused, but for another reason: $($_.Exception.Message)"
        }
        return
    }
    throw "Expected a refusal matching '$Pattern'; nothing was refused."
}

function Start-Ping([string]$Path, [int]$Count = 60) {
    return Start-Process -FilePath $Path -ArgumentList '-n', $Count, '127.0.0.1' -PassThru -WindowStyle Hidden
}

$id = [guid]::NewGuid().ToString('N').Substring(0, 8)
$temp = Get-NormalizedPath $env:TEMP
$case = Join-Path $temp "release-qa-checks-$id"
$outside = Join-Path $repo "scratch\release-qa-checks-$id"
$registryRoot = "HKCU:\Software\SakatsuReleaseQaChecks-$id"
$emptyKeys = @("$registryRoot\Empty")
$ping = Join-Path $env:windir 'System32\PING.EXE'
New-Item -ItemType Directory -Path $case | Out-Null
New-Item -ItemType Directory -Path $outside | Out-Null
New-Item -Path "$registryRoot\Empty" -Force | Out-Null
$dummyInstaller = Join-Path $case 'dummy-setup.exe'
Copy-Item -LiteralPath $ping -Destination $dummyInstaller

Write-Host 'Release installer harness checks'
try {
    Test-Case 'a live process and a runtime error dialog never count as the web interface' {
        $owned = [pscustomobject]@{ Id = 0; Process = [pscustomobject]@{ HasExited = $false } }
        $dialog = @([pscustomobject]@{ Type = 'ControlType.Text'; Name = 'Could not find the WebView2 Runtime'; Offscreen = $false })
        Assert-Throws { Wait-InstalledInterface $owned -TimeoutSeconds 0 -ReadSnapshot { $dialog } } 'runtime error'
        Assert-Throws { Wait-InstalledInterface $owned -TimeoutSeconds 0 -ReadSnapshot { @() } } 'no Sakatsu web interface'
        $settingsOnly = @([pscustomobject]@{ Type = 'ControlType.Button'; Name = 'Settings'; Offscreen = $false })
        Assert-Throws { Wait-InstalledInterface $owned -TimeoutSeconds 0 -ReadSnapshot { $settingsOnly } } 'no Sakatsu web interface'
        $webOnly = @([pscustomobject]@{ Type = 'ControlType.Document'; Name = 'Sakatsu Reader'; Offscreen = $false })
        Assert-Throws { Wait-InstalledInterface $owned -TimeoutSeconds 0 -ReadSnapshot { $webOnly } } 'no Sakatsu web interface'
        foreach ($name in 'Read', 'Settings', 'Set up Sakatsu Reader') {
            $ready = @($webOnly[0], [pscustomobject]@{ Type = 'ControlType.Button'; Name = $name; Offscreen = $false })
            $seen = @(Wait-InstalledInterface $owned -TimeoutSeconds 0 -ReadSnapshot { $ready })
            Assert-True ($seen.Count -eq 2) 'a rendered reader or welcome page is accepted'
        }
        $hidden = @($webOnly[0], [pscustomobject]@{ Type = 'ControlType.Button'; Name = 'Settings'; Offscreen = $true })
        Assert-Throws { Wait-InstalledInterface $owned -TimeoutSeconds 0 -ReadSnapshot { $hidden } } 'no Sakatsu web interface'
        $exited = [pscustomobject]@{ Id = 0; Process = [pscustomobject]@{ HasExited = $true; ExitCode = 1 } }
        Assert-Throws { Wait-InstalledInterface $exited -TimeoutSeconds 0 -ReadSnapshot { $ready } } 'exited before rendering'
    }

    Test-Case 'containment is by path component, not by string prefix' {
        Assert-True (Test-PathWithin 'C:\work\Temp\qa' 'C:\work\Temp') 'a child is inside'
        Assert-True (-not (Test-PathWithin 'C:\work\Temp2\qa' 'C:\work\Temp')) 'a sibling sharing the prefix is not inside'
        Assert-True (-not (Test-PathWithin 'C:\work\Tempest' 'C:\work\Temp')) 'a longer name is not inside'
        Assert-True (Test-PathWithin 'c:/work/temp/QA/' 'C:\work\Temp\') 'case, slashes and trailing separators do not matter'
        Assert-True (-not (Test-PathWithin 'C:\work\Temp\qa\..\..\Temp2' 'C:\work\Temp')) 'dot-dot is resolved before comparing'
    }

    Test-Case 'an 8.3 short name and its long form are the same folder' {
        $short = (New-Object -ComObject Scripting.FileSystemObject).GetFolder($case).ShortPath
        if ($short -eq $case) {
            Write-Host '        (this volume has no 8.3 names; the comparison is trivially the same)'
        }
        Assert-True ((Get-NormalizedPath $short) -eq (Get-NormalizedPath $case)) "$short and $case normalise differently"
        Assert-True (Test-PathWithin (Join-Path $short 'qa') $case) 'a short-form child is inside the long-form folder'
        Assert-True (Test-PathWithin (Join-Path $case 'qa') $short) 'a long-form child is inside the short-form folder'
    }

    Test-Case 'a folder is never inside itself' {
        Assert-True (-not (Test-PathWithin 'C:\work\Temp' 'C:\work\Temp\')) 'the base is not inside the base'
    }

    Test-Case 'the QA root must be strictly inside TEMP' {
        Assert-Throws { Resolve-QaRoot $temp } 'must be inside'
        Assert-Throws { Resolve-QaRoot ($temp + '2\qa') } 'must be inside'
        $good = Resolve-QaRoot (Join-Path $case 'qa')
        Assert-True ($good -eq (Join-Path $case 'qa')) "a folder inside TEMP resolves to itself, not $good"
    }

    Test-Case 'a junction cannot carry the QA root out of TEMP' {
        $link = Join-Path $case 'link'
        cmd /c mklink /J "$link" "$outside" | Out-Null
        try {
            Assert-True (Test-Path -LiteralPath $link) 'the junction was created'
            Assert-Throws { Resolve-QaRoot (Join-Path $link 'qa') } 'junction'
        } finally {
            # rmdir on a junction removes the link alone.
            cmd /c rmdir "$link" | Out-Null
        }
    }

    Test-Case 'removing an owned folder removes a junction inside it, never its target' {
        $owned = Join-Path $case 'owned'
        New-Item -ItemType Directory -Path $owned | Out-Null
        $keep = Join-Path $outside 'keep.txt'
        Set-Content -LiteralPath $keep -Value 'must survive'
        cmd /c mklink /J "$(Join-Path $owned 'link')" "$outside" | Out-Null
        Set-Content -LiteralPath (Join-Path $owned 'file.txt') -Value 'owned'
        Remove-OwnedFolder $owned $temp
        Assert-True (-not (Test-Path -LiteralPath $owned)) 'the owned folder is gone'
        Assert-True (Test-Path -LiteralPath $keep) 'the junction target kept its contents'
    }

    Test-Case 'every profile variable must be inside the run folder' {
        $run = Join-Path $case 'run'
        $environment = New-IsolatedEnvironment $run
        Assert-IsolatedEnvironment $environment $run
        $escaped = New-IsolatedEnvironment $run
        $escaped['APPDATA'] = $env:APPDATA
        Assert-Throws { Assert-IsolatedEnvironment $escaped $run } 'APPDATA must be inside'
        $sibling = New-IsolatedEnvironment $run
        $sibling['LOCALAPPDATA'] = $run + '2\Local'
        Assert-Throws { Assert-IsolatedEnvironment $sibling $run } 'LOCALAPPDATA must be inside'
        $missing = New-IsolatedEnvironment $run
        $missing.Remove('WEBVIEW2_USER_DATA_FOLDER')
        Assert-Throws { Assert-IsolatedEnvironment $missing $run } 'does not set WEBVIEW2_USER_DATA_FOLDER'
    }

    Test-Case 'a stop reaches the started process and its children, and nothing else' {
        $run = Join-Path $case 'tree'
        $environment = New-IsolatedEnvironment $run
        $unrelated = Start-Ping $ping
        try {
            # cmd.exe starts ping as its child: a two-level tree the run owns.
            $owned = Start-IsolatedProcess (Join-Path $env:windir 'System32\cmd.exe') $environment $run "/c `"$ping`" -n 60 127.0.0.1"
            Start-Sleep -Seconds 2
            $child = @(Get-CimInstance Win32_Process | Where-Object { $_.ParentProcessId -eq $owned.Id })
            Assert-True ($child.Count -ge 1) 'the owned process started a child'
            [void](Stop-OwnedProcessTree $owned)
            # Whether the stop reached it or it exited when its child did, it must be gone.
            Assert-True ($owned.Process.WaitForExit(5000)) 'the started process is no longer running'
            foreach ($each in $child) {
                Assert-True (-not (Get-Process -Id $each.ProcessId -ErrorAction SilentlyContinue)) "its child $($each.ProcessId) was stopped"
            }
            Assert-True (-not $unrelated.HasExited) 'an unrelated process with nothing to do with the run is still running'
        } finally {
            Stop-Process -Id $unrelated.Id -Force -ErrorAction SilentlyContinue
        }
    }

    Test-Case 'a running copy of Sakatsu is refused and left running' {
        $bin = Join-Path $case 'bin'
        New-Item -ItemType Directory -Force -Path $bin | Out-Null
        $impostor = Join-Path $bin 'sakatsu-desktop.exe'
        Copy-Item -LiteralPath $ping -Destination $impostor
        $running = Start-Ping $impostor
        try {
            Start-Sleep -Milliseconds 500
            Assert-Throws {
                & $harness -Installer $dummyInstaller -QaRoot (Join-Path $case 'qa') -ValidateOnly -UninstallKeys $emptyKeys | Out-Null
            } "already running: .*pid $($running.Id)"
            Assert-True (-not $running.HasExited) 'the running copy was not stopped'
        } finally {
            Stop-Process -Id $running.Id -Force -ErrorAction SilentlyContinue
            $running.WaitForExit(5000) | Out-Null
        }
    }

    $realCopyRunning = @(Get-SakatsuProcess).Count -gt 0
    if ($realCopyRunning) {
        Write-Host '  skip  the remaining harness cases: a real copy of Sakatsu is running, so every one would be refused for that reason.' -ForegroundColor Yellow
    } else {
        Test-Case 'an installed Sakatsu is refused' {
            $listed = "$registryRoot\Listed"
            $entry = Join-Path $listed 'Sakatsu Reader'
            New-Item -Path $entry -Force | Out-Null
            New-ItemProperty -Path $entry -Name DisplayName -Value 'Sakatsu Reader' | Out-Null
            New-ItemProperty -Path $entry -Name DisplayVersion -Value '0.0.1' | Out-Null
            Assert-Throws {
                & $harness -Installer $dummyInstaller -QaRoot (Join-Path $case 'qa') -ValidateOnly -UninstallKeys @($listed) | Out-Null
            } 'Windows lists an installed Sakatsu \(Sakatsu Reader 0\.0\.1\)'
        }

        Test-Case 'installing needs -DisposableEnvironment, and a refusal creates nothing' {
            $qa = Join-Path $case 'qa-refused'
            Assert-Throws {
                & $harness -Installer $dummyInstaller -QaRoot $qa -UninstallKeys $emptyKeys | Out-Null
            } 'DisposableEnvironment'
            Assert-True (-not (Test-Path -LiteralPath $qa)) 'no QA folder was created'
        }

        Test-Case 'preflight passes with nothing running or installed, and installs nothing' {
            $qa = Join-Path $case 'qa-preflight'
            & $harness -Installer $dummyInstaller -QaRoot $qa -ValidateOnly -UninstallKeys $emptyKeys | Out-Null
            Assert-True (-not (Test-Path -LiteralPath $qa)) 'validation created no QA folder'
        }
    }

    Test-Case 'the QA root is checked before anything else' {
        Assert-Throws {
            & $harness -Installer $dummyInstaller -QaRoot $temp -ValidateOnly -UninstallKeys $emptyKeys | Out-Null
        } 'must be inside'
    }
} finally {
    if (Test-Path -LiteralPath $registryRoot) { Remove-Item -LiteralPath $registryRoot -Recurse -Force }
    Remove-OwnedFolder $case $temp
    Remove-OwnedFolder $outside (Join-Path $repo 'scratch')
}

if ($failures -gt 0) {
    Write-Host "$failures check(s) failed." -ForegroundColor Red
    exit 1
}
Write-Host 'All release installer harness checks passed.' -ForegroundColor Green
