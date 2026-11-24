param([string]$Page = "map", [string]$Out = "shots/v17-map.png", [int]$Wait = 7)
$env:MSFS_TRACKER_PAGE = $Page
$p = Start-Process -FilePath '.\target\debug\msfs-flight-tracker.exe' -ArgumentList '--demo' -PassThru
Start-Sleep -Seconds $Wait
& powershell -NoProfile -ExecutionPolicy Bypass -File tools/winshot.ps1 -Process 'msfs-flight-tracker' -Out $Out -Wait 8
Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
