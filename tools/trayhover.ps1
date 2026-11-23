# Apre la tray, passa sopra all'icona e fotografa il tooltip.
param([int]$X = 1498, [int]$Y = 922)
$ErrorActionPreference = "Stop"
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class Th {
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindow(string cls, string name);
}
"@
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms

function Open-Flyout {
  $t = [System.Windows.Automation.AutomationElement]::FromHandle([Th]::FindWindow("Shell_TrayWnd", $null))
  $all = $t.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
  foreach ($el in $all) {
    if ($el.Current.Name -like "*icone nascoste*" -or $el.Current.Name -like "*hidden icons*") {
      $r = $el.Current.BoundingRectangle
      [Th]::SetCursorPos([int]($r.X + $r.Width / 2), [int]($r.Y + $r.Height / 2)) | Out-Null
      Start-Sleep -Milliseconds 200
      Add-Type -AssemblyName System.Windows.Forms
      $sig = '[DllImport("user32.dll")] public static extern void mouse_event(uint f, int dx, int dy, uint d, int e);'
      $type = Add-Type -MemberDefinition $sig -Name "MouseFly" -Namespace "Win" -PassThru
      $type::mouse_event(0x0002, 0, 0, 0, 0)
      $type::mouse_event(0x0004, 0, 0, 0, 0)
      Start-Sleep -Milliseconds 1500
      return
    }
  }
}

Get-Process msfs-flight-tracker -ErrorAction SilentlyContinue |
  Where-Object { $_.Path -like "*target\release*" } | Stop-Process -Force
Start-Sleep -Seconds 2

$exe = Join-Path $PSScriptRoot "..\target\release\msfs-flight-tracker.exe"
$p = Start-Process -FilePath $exe -ArgumentList "--demo" -PassThru
Start-Sleep -Seconds 9
$p.Refresh()
$hwnd = $p.MainWindowHandle
[Th]::PostMessage($hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
Start-Sleep -Seconds 2
"nascosta: visible=$([Th]::IsWindowVisible($hwnd))"

Open-Flyout
[Th]::SetCursorPos($X, $Y) | Out-Null
Start-Sleep -Milliseconds 1600

$bmp = New-Object System.Drawing.Bitmap 1920, 1080
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen(0, 0, 0, 0, $bmp.Size)
$g.Dispose()
$bmp.Save((Join-Path $PSScriptRoot "..\shots\tray_hover.png"), [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
"hover fatto su $X,$Y"
Stop-Process -Id $p.Id -Force
