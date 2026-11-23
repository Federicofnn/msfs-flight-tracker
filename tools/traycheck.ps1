# Verifica: la X nasconde la finestra (via dalla barra) senza chiudere l'app,
# e l'interfaccia continua a girare mentre e' nascosta.
$ErrorActionPreference = "Stop"
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class Tr {
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
}
"@

$exe = Join-Path $PSScriptRoot "..\target\release\msfs-flight-tracker.exe"
$p = Start-Process -FilePath $exe -ArgumentList "--demo" -PassThru
Start-Sleep -Seconds 9
$p.Refresh()
$hwnd = $p.MainWindowHandle
"start: pid=$($p.Id) hwnd=$hwnd visible=$([Tr]::IsWindowVisible($hwnd)) alive=$(-not $p.HasExited)"

[Tr]::PostMessage($hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
Start-Sleep -Seconds 3
$p.Refresh()
"after X: visible=$([Tr]::IsWindowVisible($hwnd)) alive=$(-not $p.HasExited)"

# Seconda X su una finestra gia' nascosta: se l'app resta viva, l'interfaccia
# gira ancora mentre e' nascosta (quindi i click sulla tray vengono gestiti).
[Tr]::PostMessage($hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
Start-Sleep -Seconds 3
$p.Refresh()
"after 2nd X: visible=$([Tr]::IsWindowVisible($hwnd)) alive=$(-not $p.HasExited)"

# L'icona nella tray esiste?
Add-Type -AssemblyName UIAutomationClient
$root = [System.Windows.Automation.AutomationElement]::RootElement
$all = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants,
  [System.Windows.Automation.Condition]::TrueCondition)
$found = $null
foreach ($el in $all) {
  $name = $el.Current.Name
  if ($name -like "*MSFS Flight Tracker*" -and $el.Current.ControlType.ProgrammaticName -eq "ControlType.Button") { $found = $el; break }
}
if ($found) {
  "tray: icona presente a $($found.Current.BoundingRectangle)"
} else {
  "tray: icona non raggiungibile via UIA (probabilmente nella freccia delle icone nascoste)"
}

Stop-Process -Id $p.Id -Force
"stopped"
