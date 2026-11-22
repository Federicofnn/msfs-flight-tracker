param(
    [string]$page = "map",
    [string]$out = "shot.png",
    [int]$wait = 8,
    [int]$scroll = 0,
    [int]$clickX = 0,
    [int]$clickY = 0,
    [int]$tab = 0
)
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class Win {
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, int data, UIntPtr extra);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hwnd, IntPtr hdc, uint flags);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
}
"@

[Win]::SetProcessDPIAware() | Out-Null
$env:MSFS_TRACKER_PAGE = $page
if ($tab) { $env:MSFS_TRACKER_TAB = "$tab" } else { Remove-Item Env:MSFS_TRACKER_TAB -ErrorAction SilentlyContinue }
$proc = Start-Process -FilePath "dist\msfs-flight-tracker.exe" -ArgumentList "--demo" -PassThru
Start-Sleep -Seconds $wait
$proc.Refresh()
$handle = $proc.MainWindowHandle
[Win]::SetForegroundWindow($handle) | Out-Null
$rectPre = New-Object Win+RECT
[Win]::GetWindowRect($handle, [ref]$rectPre) | Out-Null
if ($clickX -ne 0 -and $clickY -ne 0) {
    [Win]::SetCursorPos($clickX, $clickY) | Out-Null
    Start-Sleep -Milliseconds 150
    [Win]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 60
    [Win]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 400
}
if ($scroll -ne 0) {
    $steps = [math]::Ceiling([math]::Abs($scroll) / 120.0)
    $delta = if ($scroll -lt 0) { -120 } else { 120 }
    [Win]::SetCursorPos([int](($rectPre.Left + $rectPre.Right) / 2), [int](($rectPre.Top + $rectPre.Bottom) / 2)) | Out-Null
    for ($i = 0; $i -lt $steps; $i++) {
        [Win]::mouse_event(0x0800, 0, 0, $delta, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 30
    }
}
Start-Sleep -Milliseconds 900

$rect = New-Object Win+RECT
[Win]::GetWindowRect($handle, [ref]$rect) | Out-Null
$width = $rect.Right - $rect.Left
$height = $rect.Bottom - $rect.Top
$bitmap = New-Object System.Drawing.Bitmap $width, $height
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$hdc = $graphics.GetHdc()
$printed = [Win]::PrintWindow($handle, $hdc, 2)
$graphics.ReleaseHdc($hdc)
if (-not $printed) {
    $graphics.CopyFromScreen($rect.Left, $rect.Top, 0, 0, $bitmap.Size)
}
$bitmap.Save((Join-Path (Get-Location) $out), [System.Drawing.Imaging.ImageFormat]::Png)
$graphics.Dispose()
$bitmap.Dispose()
Write-Output "saved $out ${width}x${height}"
$proc.Refresh()
if (-not $proc.HasExited) { Stop-Process -Id $proc.Id -Force }
