# Builds the app and the setup, then writes the update manifest.
#
#   powershell -ExecutionPolicy Bypass -File tools/release.ps1 `
#       -BaseUrl "https://tuo-sito.example/releases/1.2.0" `
#       -Notes "Foto aereo corretta, traffico VATSIM/IVAO, nuovo tema."
#
# Upload dist/MSFSFlightTracker-Setup-<version>.exe and dist/release/latest.json
# to that folder; every installed app reads latest.json and updates itself.
param(
    [string]$BaseUrl = "",
    [string]$Notes = ""
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$version = (Select-String -Path "Cargo.toml" -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
Write-Host "Building MSFS Flight Tracker $version"

cargo build --release
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

New-Item -ItemType Directory -Force -Path dist, dist\release | Out-Null
Copy-Item target\release\msfs-flight-tracker.exe dist\msfs-flight-tracker.exe -Force
if (Test-Path "SimConnect.dll") { Copy-Item SimConnect.dll dist\SimConnect.dll -Force }

Write-Host "Building the installer"
cargo build --release --manifest-path installer/Cargo.toml
if ($LASTEXITCODE -ne 0) { throw "installer build failed" }

$setup = "dist\MSFSFlightTracker-Setup-$version.exe"
Copy-Item installer\target\release\msfs-flight-tracker-setup.exe $setup -Force

$url = if ($BaseUrl) {
    "$($BaseUrl.TrimEnd('/'))/MSFSFlightTracker-Setup-$version.exe"
} else {
    "https://IL-TUO-SITO/releases/MSFSFlightTracker-Setup-$version.exe"
}
$manifest = [ordered]@{ version = $version; url = $url; notes = $Notes } | ConvertTo-Json
Set-Content -Path "dist\release\latest.json" -Value $manifest -Encoding UTF8

$appSize = [math]::Round((Get-Item dist\msfs-flight-tracker.exe).Length / 1MB, 2)
$setupSize = [math]::Round((Get-Item $setup).Length / 1MB, 2)
Write-Host ""
Write-Host "App:     dist\msfs-flight-tracker.exe ($appSize MB)"
Write-Host "Setup:   $setup ($setupSize MB)"
Write-Host "Manifest: dist\release\latest.json"
Get-Content dist\release\latest.json
