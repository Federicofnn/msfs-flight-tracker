# Captures one window by title into a PNG (used to check the setup window).
#
#   powershell -ExecutionPolicy Bypass -File tools/winshot.ps1 `
#       -Title "MSFS Flight Tracker Setup" -Out shots/setup.png -Wait 6
param(
    [string]$Title = "",
    [string]$Process = "",
    [Parameter(Mandatory = $true)][string]$Out,
    [int]$Wait = 5
)

$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing

Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class WinShot {
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern IntPtr FindWindow(string cls, string title);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out RECT rect);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hwnd, IntPtr hdc, uint flags);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hwnd, int cmd);
    public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
}
"@

[WinShot]::SetProcessDPIAware() | Out-Null
$hwnd = [IntPtr]::Zero
for ($i = 0; $i -lt ($Wait * 4); $i++) {
    if ($Process) {
        $proc = Get-Process -Name $Process -ErrorAction SilentlyContinue |
            Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
        if ($proc) { $hwnd = $proc.MainWindowHandle }
    }
    if ($hwnd -eq [IntPtr]::Zero -and $Title) {
        $hwnd = [WinShot]::FindWindow($null, $Title)
    }
    if ($hwnd -ne [IntPtr]::Zero) { break }
    Start-Sleep -Milliseconds 250
}
if ($hwnd -eq [IntPtr]::Zero) { throw "Window not found ($Title $Process)" }

[WinShot]::ShowWindow($hwnd, 9) | Out-Null
[WinShot]::SetForegroundWindow($hwnd) | Out-Null
Start-Sleep -Milliseconds 900

$rect = New-Object WinShot+RECT
[WinShot]::GetWindowRect($hwnd, [ref]$rect) | Out-Null
$width = $rect.Right - $rect.Left
$height = $rect.Bottom - $rect.Top
$bitmap = New-Object System.Drawing.Bitmap $width, $height
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$hdc = $graphics.GetHdc()
$ok = [WinShot]::PrintWindow($hwnd, $hdc, 2)
$graphics.ReleaseHdc($hdc)
$graphics.Dispose()
if (-not $ok) { throw "PrintWindow failed" }

$path = Join-Path (Get-Location) $Out
$folder = Split-Path -Parent $path
if ($folder -and -not (Test-Path $folder)) { New-Item -ItemType Directory -Force -Path $folder | Out-Null }
$bitmap.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
$bitmap.Dispose()
Write-Host "saved $Out ${width}x${height}"
