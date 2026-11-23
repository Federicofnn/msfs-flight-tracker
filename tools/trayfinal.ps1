# Test definitivo: X -> nascosta; apre la tray; chiede a Windows la posizione
# esatta dell'icona; la clicca; verifica che la finestra torni.
$ErrorActionPreference = "Stop"
Add-Type @"
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public class Fi {
  [StructLayout(LayoutKind.Sequential)]
  public struct NOTIFYICONIDENTIFIER { public int cbSize; public IntPtr hWnd; public uint uID; public Guid guidItem; }
  [StructLayout(LayoutKind.Sequential)]
  public struct RECT { public int Left, Top, Right, Bottom; }
  [DllImport("shell32.dll")] public static extern int Shell_NotifyIconGetRect(ref NOTIFYICONIDENTIFIER id, out RECT rect);
  public delegate bool EnumProc(IntPtr h, IntPtr p);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr p);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int dx, int dy, uint d, int e);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindow(string cls, string name);
  public static string Cls(IntPtr h) { var sb = new StringBuilder(256); GetClassName(h, sb, 256); return sb.ToString(); }
  public static IntPtr TrayWindow(uint pid) {
    IntPtr found = IntPtr.Zero;
    EnumWindows((h, p) => { uint owner; GetWindowThreadProcessId(h, out owner); if (owner == pid && Cls(h) == "tray_icon_app") found = h; return true; }, IntPtr.Zero);
    return found;
  }
  public static string Rect(uint pid) {
    var h = TrayWindow(pid);
    if (h == IntPtr.Zero) return "no-tray-window";
    for (uint uid = 0; uid <= 40; uid++) {
      var id = new NOTIFYICONIDENTIFIER();
      id.cbSize = Marshal.SizeOf(id); id.hWnd = h; id.uID = uid; id.guidItem = Guid.Empty;
      RECT r;
      if (Shell_NotifyIconGetRect(ref id, out r) == 0) return r.Left + "," + r.Top + "," + r.Right + "," + r.Bottom;
    }
    return "no-rect";
  }
}
"@
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName System.Drawing

function Click-At($x, $y) {
  [Fi]::SetCursorPos([int]$x, [int]$y) | Out-Null
  Start-Sleep -Milliseconds 250
  [Fi]::mouse_event(0x0002, 0, 0, 0, 0)
  [Fi]::mouse_event(0x0004, 0, 0, 0, 0)
}
function Open-Flyout {
  $t = [System.Windows.Automation.AutomationElement]::FromHandle([Fi]::FindWindow("Shell_TrayWnd", $null))
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
function Click-MenuItem($name) {
  $root = [System.Windows.Automation.AutomationElement]::RootElement
  $cond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, $name)
  try { $el = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $cond) } catch { return $false }
  if (-not $el) { return $false }
  $r = $el.Current.BoundingRectangle
  Click-At ($r.X + $r.Width / 2) ($r.Y + $r.Height / 2)
  return $true
}
function Snap($name) {
  $bmp = New-Object System.Drawing.Bitmap 1920, 1080
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen(0, 0, 0, 0, $bmp.Size)
  $g.Dispose()
  $bmp.Save((Join-Path $PSScriptRoot "..\shots\$name"), [System.Drawing.Imaging.ImageFormat]::Png)
  $bmp.Dispose()
}

Get-Process msfs-flight-tracker -ErrorAction SilentlyContinue |
  Where-Object { $_.Path -like "*target\release*" } | Stop-Process -Force
Start-Sleep -Seconds 2

$p = Start-Process -FilePath (Join-Path $PSScriptRoot "..\target\release\msfs-flight-tracker.exe") -ArgumentList "--demo" -PassThru
Start-Sleep -Seconds 9
$p.Refresh()
$hwnd = $p.MainWindowHandle
[Fi]::PostMessage($hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
Start-Sleep -Seconds 2
"dopo X: visible=$([Fi]::IsWindowVisible($hwnd))"
"rect icona (tray chiusa): $([Fi]::Rect([uint32]$p.Id))"

Open-Flyout
$rect = [Fi]::Rect([uint32]$p.Id)
"rect icona (tray aperta): $rect"
Snap "tray_final.png"

if ($rect -match "^(\d+),(\d+),(\d+),(\d+)$") {
  $cx = ([int]$matches[1] + [int]$matches[3]) / 2
  $cy = ([int]$matches[2] + [int]$matches[4]) / 2
  Click-At $cx $cy
  Start-Sleep -Milliseconds 1200
  Snap "tray_menu2.png"
  $opened = Click-MenuItem "Apri MSFS Flight Tracker"
  Start-Sleep -Milliseconds 1800
  "click icona a $cx,$cy -> menu=$opened -> visible=$([Fi]::IsWindowVisible($hwnd))"
} else {
  "posizione non disponibile"
}
Stop-Process -Id $p.Id -Force
