<#
.SYNOPSIS
    Closing the reader must end the session, even with the capture frame up.

.DESCRIPTION
    The bug this was written after: closing the reader while the frame was open left the process
    running with nothing able to stop it. The frame is `decorations(false)` so it has no close
    button, `skip_taskbar(true)` so it is not in the taskbar or Alt-Tab, and `always_on_top(true)`
    so it sits over everything. None of the three bindings reopens the reader. The chords went on
    working, which is what made it read as a window that had merely been hidden - and Task Manager
    was the only way out. An instance was found on the development machine in exactly that state,
    `MainWindowTitle` reading "capture frame", hours after its reader had gone.

    Not a `cargo test`: the thing under test is a Win32 `WM_CLOSE` reaching Tauri's event loop and
    a real second window outliving the first. The mock runtime only raises `CloseRequested` inside
    a run loop the unit tests do not pump, so it cannot see this at all.

    **`WM_CLOSE` is posted rather than the caption button being clicked**, because the caption
    button is the one path that was never in doubt. Alt+F4 and Close on the system menu - which
    `titlebar.rs` deliberately offers - bypass `commands::close_window` entirely, which is why the
    fix is a window-event handler and not a change to that command. This posts what they post.

    Opens the frame with whatever chord the current profile has set, read from the store rather
    than assumed. Nothing is installed and nothing is written; the app runs from `target/release`.

.EXAMPLE
    pwsh -File scripts/test-window-lifetime.ps1

.NOTES
    Run under PowerShell 7. The repository path contains characters that Windows PowerShell 5.1
    misreads from a BOM-less UTF-8 script, which fails as "cannot find the file specified".
#>
[CmdletBinding()]
param(
    [int] $ReaderTimeoutSeconds = 90,
    [int] $ExitTimeoutSeconds = 20
)
$ErrorActionPreference = 'Stop'

Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class Win32 {
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern void GetWindowThreadProcessId(IntPtr h, out uint p);
}
"@

function Get-WindowTitles([int] $ProcessId) {
    $found = New-Object System.Collections.ArrayList
    # Held in a variable so the delegate is not collected while EnumWindows is still calling it.
    $callback = [Win32+EnumProc] {
        param($handle, $lparam)
        $owner = 0
        [Win32]::GetWindowThreadProcessId($handle, [ref]$owner)
        if ($owner -eq $ProcessId) {
            $title = New-Object System.Text.StringBuilder 512
            [void][Win32]::GetWindowTextW($handle, $title, 512)
            [void]$found.Add($title.ToString())
        }
        return $true
    }
    [void][Win32]::EnumWindows($callback, [IntPtr]::Zero)
    return $found
}

$root = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $root 'target\release\sakatsu-desktop.exe'
if (-not (Test-Path $exe)) {
    throw "No binary at $exe. Run ``cargo build -p sakatsu-desktop --release`` first."
}

# A copy already running would be turned away by the single-instance plugin, and the test would
# then measure a process that exited for a completely different reason.
if (Get-Process -Name 'sakatsu-desktop' -ErrorAction SilentlyContinue) {
    throw 'Sakatsu is already running. Close it first, or this measures the wrong process.'
}

$app = Start-Process -FilePath $exe -PassThru
Write-Output "started pid=$($app.Id)"

$appeared = $false
for ($t = 1; $t -le $ReaderTimeoutSeconds; $t++) {
    Start-Sleep -Seconds 1
    $app.Refresh()
    if ($app.HasExited) { throw 'the app exited before the reader appeared' }
    if ($app.MainWindowHandle -ne [IntPtr]::Zero) { $appeared = $true; break }
}
if (-not $appeared) { $app.Kill(); throw "no reader window within ${ReaderTimeoutSeconds}s" }
Write-Output "reader after ${t}s: '$($app.MainWindowTitle)'"
$reader = $app.MainWindowHandle

Start-Sleep -Seconds 4
$settings = Join-Path $env:APPDATA 'com.sakatsu.app\store\settings.json'
$chord = 'CmdOrCtrl+Shift+E'
if (Test-Path $settings) {
    $saved = (Get-Content $settings -Raw | ConvertFrom-Json).overlay_bind
    if ($saved.chord) { $chord = $saved.chord }
}
$keys = switch ($chord) {
    'Backquote' { '`' }
    default { '^+e' }
}
Write-Output "raising the frame with $chord"
(New-Object -ComObject WScript.Shell).SendKeys($keys)
Start-Sleep -Seconds 5

$withFrame = Get-WindowTitles $app.Id
if (-not ($withFrame | Where-Object { $_ -like '*capture frame*' })) {
    $app.Kill()
    throw "the capture frame never opened - the case that matters was not tested. Windows seen: $($withFrame -join ', ')"
}
Write-Output 'the frame is up'

Write-Output 'posting WM_CLOSE to the reader'
[void][Win32]::PostMessageW($reader, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)

for ($t = 1; $t -le $ExitTimeoutSeconds; $t++) {
    Start-Sleep -Seconds 1
    $app.Refresh()
    if ($app.HasExited) {
        Write-Output "PASS: the session ended ${t}s after the reader closed, taking the frame with it"
        exit 0
    }
}

$leftover = Get-WindowTitles $app.Id
Write-Output "FAIL: still running ${ExitTimeoutSeconds}s later, MainWindowTitle='$($app.MainWindowTitle)'"
foreach ($title in $leftover) { Write-Output "  leftover: '$title'" }
$app.Kill()
throw 'closing the reader did not end the session'
