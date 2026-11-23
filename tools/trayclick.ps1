# Chiude la tray, la riapre, prende il rect esatto dell'icona, ci clicca sopra.
$ErrorActionPreference = "Stop"
Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class Tc {
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
  public static int[] Rect(uint pid) {
    var h = TrayWindow(pid);
    if (h == IntPtr.Zero) return null;
    for (uint uid = 0; uid <= 40; uid++) {
      var id = new NOTIFYICONIDENTIFIER();
      id.cbSize = Marshal.SizeOf(id); id.hWnd = h; id.uID = uid; id.guidItem = Guid.Empty;
      RECT r;
      if (Shell_NotifyIconGetRect(ref id, out r) == 0) return new int[] { r.Left, r.Top, r.Right, r.Bottom };
    }
    return null;
  }
}
"@
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName System.Windows.Forms

function Click-At($x, $y) {
  [Tc]::SetCursorPos([int]$x, [int]$y) | Out-Null
  Start-Sleep -Milliseconds 250
  [Tc]::mouse_event(0x0002, 0, 0, 0, 0)
  [Tc]::mouse_event(0x0004, 0, 0, 0, 0)
}
function Chevron {
  $t = [System.Windows.Automation.AutomationElement]::FromHandle([Tc]::FindWindow("Shell_TrayWnd", $null))
  $all = $t.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
  foreach ($el in $all) {
    if ($el.Current.Name -like "*icone nascoste*" -or $el.Current.Name -like "*hidden icons*") { return $el.Current.BoundingRectangle }
  }
  return $null
}

Get-Process msfs-flight-tracker -ErrorAction SilentlyContinue |
  Where-Object { $_.Path -like "*target\release*" } | Stop-Process -Force
Start-Sleep -Seconds 2

$p = Start-Process -FilePath (Join-Path $PSScriptRoot "..\target\release\msfs-flight-tracker.exe") -ArgumentList "--demo" -PassThru
Start-Sleep -Seconds 9
$p.Refresh()
$hwnd = $p.MainWindowHandle
[Tc]::PostMessage($hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
Start-Sleep -Seconds 2
"dopo X: visible=$([Tc]::IsWindowVisible($hwnd))"

# Chiude un eventuale flyout aperto, poi lo apre davvero.
[System.Windows.Forms.SendKeys]::SendWait("{ESC}")
Start-Sleep -Milliseconds 800
$r = [Tc]::Rect([uint32]$p.Id)
"rect iniziale: $($r -join ',')"
$c = Chevron
# Il rect del chevron (in basso a destra) significa tray chiusa: va aperta.
if ($r -and $r[0] -ge 1560 -and $c) {
  Click-At ($c.X + $c.Width / 2) ($c.Y + $c.Height / 2)
  Start-Sleep -Milliseconds 1600
  $r = [Tc]::Rect([uint32]$p.Id)
  "rect con tray aperta: $($r -join ',')"
} else {
  "tray gia' aperta"
}

if ($r) {
  Click-At ((($r[0] + $r[2]) / 2)) ((($r[1] + $r[3]) / 2))
  Start-Sleep -Milliseconds 2000
  "dopo il click sull'icona: visible=$([Tc]::IsWindowVisible($hwnd))"
}
Stop-Process -Id $p.Id -Force
