# Publishes the current version to GitHub: app + setup + update manifest.
#
#   1. bump "version" in Cargo.toml (also installer/Cargo.toml) and app.rc
#   2. powershell -ExecutionPolicy Bypass -File tools/publish.ps1 `
#          -Notes "Cosa cambia in questa versione"
#
# The app reads
#   https://github.com/Federicofnn/msfs-flight-tracker/releases/latest/download/latest.json
# so every installed copy updates itself as soon as the release is live.
param(
    [Parameter(Mandatory = $true)][string]$Notes
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$version = (Select-String -Path "Cargo.toml" -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$tag = "v$version"
$repo = "Federicofnn/msfs-flight-tracker"
$base = "https://github.com/$repo/releases/download/$tag"

Write-Host "Releasing $tag"
& powershell -NoProfile -ExecutionPolicy Bypass -File tools/release.ps1 -BaseUrl $base -Notes $Notes
if ($LASTEXITCODE -ne 0) { throw "release.ps1 failed" }

git add -A
$pending = git status --porcelain
if ($pending) {
    git commit -m "Release $version" -m $Notes
    git push
}

$assets = @("dist/MSFSFlightTracker-Setup-$version.exe", "dist/release/latest.json")
$known = gh release list --limit 100 --json tagName --jq '.[].tagName'
if ($known -contains $tag) {
    gh release upload $tag @assets --clobber
} else {
    gh release create $tag @assets --title "MSFS Flight Tracker $version" --notes $Notes
}

Write-Host ""
Write-Host "Live: https://github.com/$repo/releases/tag/$tag"
Write-Host "Feed: https://github.com/$repo/releases/latest/download/latest.json"
