# Apre la tray delle icone nascoste, fotografa il flyout e (opzionale) clicca
# le coordinate indicate per verificare che la finestra torni.
param([string]$Points = "", [switch]$Sweep, [string]$Shot = "shots\tray.png")
$ErrorActionPreference = "Stop"
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class Tr2 {
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int dx, int dy, uint d, int e);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindow(string cls, string name);
}
"@
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms

function Click-At($x, $y) {
  [Tr2]::SetCursorPos([int]$x, [int]$y) | Out-Null
  Start-Sleep -Milliseconds 200
  [Tr2]::mouse_event(0x0002, 0, 0, 0, 0)
  [Tr2]::mouse_event(0x0004, 0, 0, 0, 0)
}
function Find-In($element, $needle) {
  for ($i = 0; $i -lt 3; $i++) {
    try {
      $all = $element.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
      foreach ($el in $all) { if ($el.Current.Name -like "*$needle*") { return $el } }
      return $null
    } catch { Start-Sleep -Milliseconds 400 }
  }
  return $null
}

function Grid {
  for ($y = 905; $y -le 1010; $y += 18) {
    for ($x = 1465; $x -le 1780; $x += 18) {
      [Tr2]::SetCursorPos($x, $y) | Out-Null
      Start-Sleep -Milliseconds 70
    }
  }
}
function Open-Flyout {
  $t = [System.Windows.Automation.AutomationElement]::FromHandle([Tr2]::FindWindow("Shell_TrayWnd", $null))
  foreach ($needle in @("hidden icons", "icone nascoste", "Show hidden", "Mostra icone")) {
    $c = Find-In $t $needle
    if ($c) { $r = $c.Current.BoundingRectangle; Click-At ($r.X + $r.Width / 2) ($r.Y + $r.Height / 2); Start-Sleep -Milliseconds 1500; return $true }
  }
  return $false
}

if ($Sweep) {
  # Passa sopra tutte le icone: Windows elimina quelle morte (residui dei test).
  Open-Flyout | Out-Null
  Grid
  [System.Windows.Forms.SendKeys]::SendWait("{ESC}")
  Start-Sleep -Milliseconds 600
  "sweep fatto"
}

$exe = Join-Path $PSScriptRoot "..\target\release\msfs-flight-tracker.exe"
$p = Start-Process -FilePath $exe -ArgumentList "--demo" -PassThru
Start-Sleep -Seconds 9
$p.Refresh()
$hwnd = $p.MainWindowHandle
[Tr2]::PostMessage($hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
Start-Sleep -Seconds 2
"nascosta: visible=$([Tr2]::IsWindowVisible($hwnd))"

Open-Flyout | Out-Null

$screen = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$bmp = New-Object System.Drawing.Bitmap $screen.Width, $screen.Height
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($screen.X, $screen.Y, 0, 0, $bmp.Size)
$out = Join-Path $PSScriptRoot "..\$Shot"
$bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose()
$bmp.Dispose()
"screenshot: $out ($($screen.Width)x$($screen.Height))"

foreach ($pair in ($Points -split ";" | Where-Object { $_ })) {
  $xy = $pair -split ","
  Click-At ([int]$xy[0]) ([int]$xy[1])
  Start-Sleep -Seconds 2
  $vis = [Tr2]::IsWindowVisible($hwnd)
  "click $($xy[0]),$($xy[1]) -> visible=$vis"
  if ($vis) { break }
}
Stop-Process -Id $p.Id -Force
"stopped"
