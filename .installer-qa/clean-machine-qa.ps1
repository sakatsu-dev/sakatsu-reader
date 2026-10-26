# Runs INSIDE Windows Sandbox. Proves a clean machine can install and open the app.
#
# # Why this replaced the wizard-driving harness
#
# The previous harness clicked through the installer with UIAutomation and screenshotted the
# result. It proved a window opened; it could not say which step failed, and it mapped only the
# NSIS folder. A clean-machine install failed for ten days with `Error 1722` and it reported
# nothing, because the thing that broke was a *custom action's exit code* - visible only in an
# MSI verbose log, and never in a screenshot.
#
# So this installs silently, logs verbosely, and asserts outcomes rather than wizard steps.
#
# # Windows PowerShell 5.1
#
# Which is what Sandbox boots with. No `if` as an expression, no ternary, no `??` - all
# PowerShell 7, all parse errors here.
$ErrorActionPreference = 'Continue'
$ProgressPreference = 'SilentlyContinue'

$work = 'C:\QA'
$out = Join-Path $work ('results\clean-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Path $out -Force | Out-Null
$findings = Join-Path $out 'findings.txt'
$checks = New-Object System.Collections.ArrayList

function Note([string]$k, $v) { "$k=$v" | Add-Content -Path $findings }
function Check([string]$name, [bool]$passed, $detail) {
    $null = $checks.Add([pscustomobject]@{ name = $name; passed = $passed; detail = "$detail" })
    $mark = 'FAIL'
    if ($passed) { $mark = 'PASS' }
    "[$mark] $name - $detail" | Add-Content -Path $findings
}

function Get-WebView2 {
    $ids = @(
        'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}',
        'HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}',
        'HKCU:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
    )
    foreach ($id in $ids) {
        $pv = (Get-ItemProperty -Path $id -Name pv -ErrorAction SilentlyContinue).pv
        if ($pv) { return $pv }
    }
    return 'absent'
}

Note 'os' (Get-CimInstance Win32_OperatingSystem).Caption
Note 'powershell' $PSVersionTable.PSVersion.ToString()

# **The precondition that makes this test mean anything.** If the image already has WebView2 the
# install would pass whatever the config said, and the regression would go unseen.
$before = Get-WebView2
Note 'webview2_before' $before
Check 'Clean machine has no WebView2' ($before -eq 'absent') $before

$net = 'unreachable'
for ($i = 0; $i -lt 10; $i++) {
    try {
        $p = Test-NetConnection -ComputerName 'msedge.api.cdp.microsoft.com' -Port 443 `
            -InformationLevel Quiet -WarningAction SilentlyContinue -ErrorAction SilentlyContinue
        if ($p) { $net = 'reachable'; break }
    } catch { }
    Start-Sleep -Seconds 3
}
Note 'network' $net

$installer = Get-ChildItem 'C:\Installers' -Recurse -Include *.exe, *.msi -ErrorAction SilentlyContinue |
    Sort-Object Length -Descending | Select-Object -First 1
if (-not $installer) {
    Check 'Installer is present' $false 'nothing under C:\Installers'
    $checks | ConvertTo-Json -Depth 4 | Set-Content (Join-Path $out 'checks.json')
    'done' | Set-Content (Join-Path $out 'DONE')
    return
}
Note 'installer' $installer.Name
Note 'installer_mb' ([math]::Round($installer.Length / 1MB, 1))
Check 'Installer is present' $true $installer.Name

$started = Get-Date
if ($installer.Extension -eq '.msi') {
    $log = Join-Path $out 'msi-verbose.log'
    $proc = Start-Process -FilePath 'msiexec.exe' `
        -ArgumentList @('/i', "`"$($installer.FullName)`"", '/qn', '/norestart', '/l*v', "`"$log`"") `
        -PassThru -Wait
} else {
    $proc = Start-Process -FilePath $installer.FullName -ArgumentList '/S' -PassThru -Wait
}
Note 'exit_code' $proc.ExitCode
Note 'install_seconds' ([math]::Round(((Get-Date) - $started).TotalSeconds, 1))
Check 'Installer exits cleanly' ($proc.ExitCode -eq 0) ("exit " + $proc.ExitCode)

# An MSI log is UTF-16. Reading it as ASCII finds nothing and looks like a clean run.
$log = Join-Path $out 'msi-verbose.log'
if (Test-Path $log) {
    $text = Get-Content $log -Raw -Encoding Unicode
    $blame = ($text -split "`n") | Where-Object { $_ -match '1722|returned actual error|Return value 3' } |
        Select-Object -First 8
    if ($blame) { $blame | Set-Content (Join-Path $out 'msi-blame.txt') }
}

$after = Get-WebView2
Note 'webview2_after' $after
Check 'Installer provisioned WebView2 without network' ($after -ne 'absent') $after

$arp = @(
    'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*',
    'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*',
    'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*'
) | ForEach-Object { Get-ItemProperty $_ -ErrorAction SilentlyContinue } |
    Where-Object { $_.DisplayName -like '*Sakatsu*' }
if ($arp) {
    Note 'arp_entry' ("$($arp[0].DisplayName) $($arp[0].DisplayVersion)")
    Note 'install_location' ("$($arp[0].InstallLocation)")
}
Check 'Add or Remove Programs entry exists' ([bool]$arp) ("$($arp[0].DisplayName)")

$exe = $null
foreach ($dir in @('C:\Program Files\Sakatsu Reader', 'C:\Program Files (x86)\Sakatsu Reader',
                   (Join-Path $env:LOCALAPPDATA 'Sakatsu Reader'),
                   (Join-Path $env:LOCALAPPDATA 'Programs\Sakatsu Reader'))) {
    if (Test-Path $dir) {
        $found = Get-ChildItem $dir -Filter '*.exe' -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($found) { $exe = $found; break }
    }
}
Check 'Application executable installed' ([bool]$exe) ("$($exe.FullName)")

# **A window, not a process.** "It is running" is not what a user experiences, and an earlier
# pass reported running=yes against an empty desktop - which is also what a hung start looks like.
if ($exe) {
    $app = Start-Process -FilePath $exe.FullName -PassThru
    $windowAt = -1
    $title = ''
    for ($t = 1; $t -le 90; $t++) {
        Start-Sleep -Seconds 1
        $app.Refresh()
        if ($app.HasExited) { break }
        if ($app.MainWindowHandle -ne [IntPtr]::Zero) { $windowAt = $t; $title = $app.MainWindowTitle; break }
    }
    Note 'window_after_seconds' $windowAt
    Note 'window_title' $title
    Check 'App opens a window' ($windowAt -ge 0) ("after ${windowAt}s, titled '$title'")
    # The runtime failure surfaces as a Win32 dialog literally titled "Error".
    Check 'Window is the app, not an error dialog' ($title -ne 'Error') $title

    Start-Sleep -Seconds 6
    Add-Type -AssemblyName System.Windows.Forms
    Add-Type -AssemblyName System.Drawing
    $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($b.X, $b.Y, 0, 0, $bmp.Size)
    $bmp.Save((Join-Path $out 'first-run.png'), [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
    if (-not $app.HasExited) { $app.Kill() }
}

# **Uninstall, and what it must leave behind.** The runtime is Microsoft's to update, shared with
# every other WebView2 app on the machine, and taking it away on uninstall would break them. So the
# check is asymmetric on purpose: our entry must go, the runtime must stay.
if ($arp) {
    $uninstall = $arp[0].QuietUninstallString
    if (-not $uninstall) { $uninstall = $arp[0].UninstallString }
    Note 'uninstall_string' $uninstall
    if ($uninstall) {
        # NSIS hands the string over quoted, and sometimes with switches already attached.
        $exePath = $uninstall
        $switches = @('/S')
        if ($uninstall -match '^"([^"]+)"\s*(.*)$') {
            $exePath = $Matches[1]
            $rest = $Matches[2].Trim()
            if ($rest -and $rest -notmatch '(?i)(^|\s)/S(\s|$)') { $switches = @($rest, '/S') }
            elseif ($rest) { $switches = @($rest) }
        }
        Start-Process -FilePath $exePath -ArgumentList $switches -ErrorAction SilentlyContinue | Out-Null

        # An NSIS uninstaller relaunches itself from %TEMP% and the first process returns at once,
        # so waiting on an exit code proves nothing. Wait for the *entry* to disappear instead.
        $gone = $false
        for ($t = 1; $t -le 90; $t++) {
            Start-Sleep -Seconds 1
            $still = @(
                'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*',
                'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*',
                'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*'
            ) | ForEach-Object { Get-ItemProperty $_ -ErrorAction SilentlyContinue } |
                Where-Object { $_.DisplayName -like '*Sakatsu*' }
            if (-not $still) { $gone = $true; break }
        }
        Note 'uninstall_seconds' $t
        Check 'Uninstall removes the Add or Remove Programs entry' $gone ''

        $exeLeft = $false
        if ($exe) { $exeLeft = Test-Path $exe.FullName }
        Check 'Uninstall removes the application executable' (-not $exeLeft) "$($exe.FullName)"

        $wv = Get-WebView2
        Note 'webview2_after_uninstall' $wv
        Check 'Uninstall leaves the WebView2 runtime installed' ($wv -ne 'absent') $wv
    }
}

$failed = @($checks | Where-Object { -not $_.passed }).Count
Note 'checks' $checks.Count
Note 'failures' $failed
$checks | ConvertTo-Json -Depth 4 | Set-Content (Join-Path $out 'checks.json')
'done' | Set-Content (Join-Path $out 'DONE')
