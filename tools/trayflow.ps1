# Giro completo: avvia, X -> nascosta, apri tray, trova l'icona rossa dell'app
# nello screenshot, la clicca e verifica che la finestra torni.
param([string]$Points = "", [switch]$Sweep)
$ErrorActionPreference = "Stop"
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class Tf {
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

$shotW = 1920; $shotH = 1080
function Click-At($x, $y) {
  [Tf]::SetCursorPos([int]$x, [int]$y) | Out-Null
  Start-Sleep -Milliseconds 250
  [Tf]::mouse_event(0x0002, 0, 0, 0, 0)
  [Tf]::mouse_event(0x0004, 0, 0, 0, 0)
}
function Grab {
  $bmp = New-Object System.Drawing.Bitmap $shotW, $shotH
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen(0, 0, 0, 0, $bmp.Size)
  $g.Dispose()
  return $bmp
}
function Save-Shot($bmp, $name) {
  $bmp.Save((Join-Path $PSScriptRoot "..\shots\$name"), [System.Drawing.Imaging.ImageFormat]::Png)
}
function Open-Flyout {
  $t = [System.Windows.Automation.AutomationElement]::FromHandle([Tf]::FindWindow("Shell_TrayWnd", $null))
  $all = $t.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
  foreach ($el in $all) {
    if ($el.Current.Name -like "*icone nascoste*" -or $el.Current.Name -like "*hidden icons*") {
      $r = $el.Current.BoundingRectangle
      Click-At ($r.X + $r.Width / 2) ($r.Y + $r.Height / 2)
      Start-Sleep -Milliseconds 1500
      return
    }
  }
}
function Flyout-Open {
  return ([Tf]::FindWindow("TopLevelWindowForOverflowXamlIsland", $null) -ne [IntPtr]::Zero) -or
         ([Tf]::FindWindow("NotifyIconOverflowWindow", $null) -ne [IntPtr]::Zero)
}
# Centri delle macchie rosse (icona dell'app) nella zona del flyout.
function Red-Icons($bmp) {
  $cells = @{}
  for ($y = 890; $y -lt 1020; $y += 4) {
    for ($x = 1450; $x -lt 1790; $x += 4) {
      $c = $bmp.GetPixel($x, $y)
      if ($c.R -gt 70 -and ($c.R - $c.G) -gt 35 -and ($c.R - $c.B) -gt 25) {
        $key = "$([int](($x - 1450) / 34)):$([int](($y - 890) / 60))"
        if (-not $cells.ContainsKey($key)) { $cells[$key] = @{ X = 0; Y = 0; N = 0 } }
        $cells[$key].X += $x; $cells[$key].Y += $y; $cells[$key].N++
      }
    }
  }
  return $cells.Values | Where-Object { $_.N -ge 6 } | ForEach-Object { [pscustomobject]@{ X = [int]($_.X / $_.N); Y = [int]($_.Y / $_.N) } }
}

Get-Process msfs-flight-tracker -ErrorAction SilentlyContinue |
  Where-Object { $_.Path -like "*target\release*" } | Stop-Process -Force
Start-Sleep -Seconds 2

if ($Sweep) {
  # Passa sopra ogni cella del flyout: Windows elimina le icone morte dei test.
  Open-Flyout
  foreach ($y in @(922, 996)) {
    foreach ($x in @(1498, 1550, 1605, 1660, 1720)) {
      [Tf]::SetCursorPos($x, $y) | Out-Null
      Start-Sleep -Milliseconds 400
    }
  }
  [System.Windows.Forms.SendKeys]::SendWait("{ESC}")
  Start-Sleep -Milliseconds 700
  "sweep fatto"
}

$exe = Join-Path $PSScriptRoot "..\target\release\msfs-flight-tracker.exe"
$p = Start-Process -FilePath $exe -ArgumentList "--demo" -PassThru
Start-Sleep -Seconds 9
$p.Refresh()
$hwnd = $p.MainWindowHandle
"avviata: visible=$([Tf]::IsWindowVisible($hwnd))"
[Tf]::PostMessage($hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
Start-Sleep -Seconds 2
"dopo X: visible=$([Tf]::IsWindowVisible($hwnd)) alive=$(-not $p.HasExited)"

Open-Flyout
for ($i = 0; $i -lt 4; $i++) {
  Start-Sleep -Milliseconds 800
  $probe = Grab
  $c = $probe.GetPixel(1475, 895)
  $probe.Dispose()
  $open = ([Math]::Abs($c.R - $c.G) -lt 8) -and ([Math]::Abs($c.G - $c.B) -lt 8) -and ($c.R -gt 24) -and ($c.R -lt 46)
  "tentativo tray $i : flyout=$open px=$($c.R),$($c.G),$($c.B)"
  if ($open) { break }
  Open-Flyout
}
$bmp = Grab
Save-Shot $bmp "tray_flow.png"
$icons = @(Red-Icons $bmp)
"icone rosse: $(($icons | ForEach-Object { "$($_.X),$($_.Y)" }) -join ' | ')"
if ($Points) { $icons = @($Points -split ";" | Where-Object { $_ } | ForEach-Object { $xy = $_ -split ","; [pscustomobject]@{ X = [int]$xy[0]; Y = [int]$xy[1] } }) }
$bmp.Dispose()

function Click-MenuItem($name) {
  $root = [System.Windows.Automation.AutomationElement]::RootElement
  $cond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, $name)
  try { $el = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $cond) } catch { return $false }
  if (-not $el) { return $false }
  $r = $el.Current.BoundingRectangle
  Click-At ($r.X + $r.Width / 2) ($r.Y + $r.Height / 2)
  return $true
}

foreach ($icon in $icons) {
  Click-At $icon.X $icon.Y
  Start-Sleep -Milliseconds 1000
  $menu = Grab
  Save-Shot $menu "tray_menu.png"
  $menu.Dispose()
  $opened = Click-MenuItem "Apri MSFS Flight Tracker"
  Start-Sleep -Milliseconds 1800
  $vis = [Tf]::IsWindowVisible($hwnd)
  "click $($icon.X),$($icon.Y) -> menu=$opened -> visible=$vis"
  if ($vis) { $after = Grab; Save-Shot $after "tray_back.png"; $after.Dispose(); break }
}
Stop-Process -Id $p.Id -Force
"stopped"
