# Clicca i punti indicati nella tray e dice se compare una finestra dell'app.
param([string]$Points = "")
$ErrorActionPreference = "Stop"
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class Tp {
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
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
  [Tp]::SetCursorPos([int]$x, [int]$y) | Out-Null
  Start-Sleep -Milliseconds 250
  [Tp]::mouse_event(0x0002, 0, 0, 0, 0)
  [Tp]::mouse_event(0x0004, 0, 0, 0, 0)
}
function Snap($name) {
  $screen = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
  $bmp = New-Object System.Drawing.Bitmap $screen.Width, $screen.Height
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($screen.X, $screen.Y, 0, 0, $bmp.Size)
  $bmp.Save((Join-Path $PSScriptRoot "..\shots\$name"), [System.Drawing.Imaging.ImageFormat]::Png)
  $g.Dispose(); $bmp.Dispose()
}
function Open-Flyout {
  $t = [System.Windows.Automation.AutomationElement]::FromHandle([Tp]::FindWindow("Shell_TrayWnd", $null))
  $all = $t.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
  foreach ($el in $all) {
    if ($el.Current.Name -like "*icone nascoste*" -or $el.Current.Name -like "*hidden icons*") {
      $r = $el.Current.BoundingRectangle
      Click-At ($r.X + $r.Width / 2) ($r.Y + $r.Height / 2)
      Start-Sleep -Milliseconds 1500
      return
    }
  }
  "chevron non trovato"
}
function Visible-App {
  $procs = Get-Process msfs-flight-tracker -ErrorAction SilentlyContinue
  $out = @()
  foreach ($proc in $procs) { $out += "pid=$($proc.Id) hwnd=$($proc.MainWindowHandle) visible=$($proc.MainWindowHandle -ne 0)" }
  return ($out -join "; ")
}

Open-Flyout
"prima: $(Visible-App)"
foreach ($pair in ($Points -split ";" | Where-Object { $_ })) {
  $xy = $pair -split ","
  Click-At ([int]$xy[0]) ([int]$xy[1])
  Start-Sleep -Milliseconds 1500
  "click $($xy[0]),$($xy[1]) -> $(Visible-App)"
}
Snap "tray_after.png"
