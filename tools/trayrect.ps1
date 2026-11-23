# Chiede a Windows la posizione dell'icona di notifica del processo:
# serve a capire se Windows sa dove sta l'icona (nella "freccia" spesso no).
$ErrorActionPreference = "Stop"
Add-Type @"
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public class Nr {
  [StructLayout(LayoutKind.Sequential)]
  public struct NOTIFYICONIDENTIFIER {
    public int cbSize; public IntPtr hWnd; public uint uID; public Guid guidItem;
  }
  [StructLayout(LayoutKind.Sequential)]
  public struct RECT { public int Left, Top, Right, Bottom; }
  [DllImport("shell32.dll", SetLastError = true)]
  public static extern int Shell_NotifyIconGetRect(ref NOTIFYICONIDENTIFIER id, out RECT rect);
  public delegate bool EnumProc(IntPtr h, IntPtr p);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr p);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  public static List<IntPtr> WindowsOf(uint pid) {
    var list = new List<IntPtr>();
    EnumWindows((h, p) => { uint owner; GetWindowThreadProcessId(h, out owner); if (owner == pid) list.Add(h); return true; }, IntPtr.Zero);
    return list;
  }
  public static string Cls(IntPtr h) { var sb = new StringBuilder(256); GetClassName(h, sb, 256); return sb.ToString(); }
}
"@

$p = Start-Process -FilePath (Join-Path $PSScriptRoot "..\target\release\msfs-flight-tracker.exe") -ArgumentList "--demo" -PassThru
Start-Sleep -Seconds 8
$wins = [Nr]::WindowsOf([uint32]$p.Id)
"finestre del processo: $($wins.Count)"
foreach ($h in $wins) {
  $cls = [Nr]::Cls($h)
  for ($uid = 0; $uid -le 30; $uid++) {
    $id = New-Object Nr+NOTIFYICONIDENTIFIER
    $id.cbSize = [System.Runtime.InteropServices.Marshal]::SizeOf($id)
    $id.hWnd = $h
    $id.uID = $uid
    $id.guidItem = [Guid]::Empty
    $rect = New-Object Nr+RECT
    $hr = [Nr]::Shell_NotifyIconGetRect([ref]$id, [ref]$rect)
    if ($hr -eq 0) {
      "OK! classe=$cls hwnd=$h uID=$uid rect=$($rect.Left),$($rect.Top),$($rect.Right),$($rect.Bottom)"
    }
  }
}
"fine"
Stop-Process -Id $p.Id -Force
