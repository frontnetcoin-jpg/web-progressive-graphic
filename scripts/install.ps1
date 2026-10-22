# WPG Installer for Windows PowerShell
$ErrorActionPreference = 'Stop'

Write-Host "⚡ Installing WPG (Web Progressive Graphic) CLI..." -ForegroundColor Cyan

$InstallDir = Join-Path $env:LOCALAPPDATA "Programs\wpg\bin"
if (!(Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
}

$CurrentDir = Get-Location
$ReleaseExe = Join-Path $CurrentDir "target\release\wpg.exe"

if (Test-Path $ReleaseExe) {
    Copy-Item $ReleaseExe -Destination $InstallDir -Force
    Write-Host "✓ Installed local release binary to: $InstallDir\wpg.exe" -ForegroundColor Green
} else {
    Write-Host "Downloading latest release binary from GitHub..." -ForegroundColor Yellow
    $DownloadUrl = "https://github.com/wpg-format/wpg/releases/latest/download/wpg-windows-x86_64.exe"
    Invoke-WebRequest -Uri $DownloadUrl -OutFile "$InstallDir\wpg.exe"
}

# Add to user PATH if not present
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -notlike "*$InstallDir*") {
    [Environment]::SetEnvironmentVariable("Path", "$UserPath;$InstallDir", "User")
    Write-Host "✓ Added $InstallDir to user PATH" -ForegroundColor Green
    Write-Host "  (Please restart your terminal or run: `$env:Path += ';$InstallDir')" -ForegroundColor Gray
}

Write-Host "`n🎉 WPG is ready! Try running:" -ForegroundColor Cyan
Write-Host "  wpg --help" -ForegroundColor White
Write-Host "  wpg serve --port 3000" -ForegroundColor White
Write-Host "  wpg batch ./photos -q 75 -r`n" -ForegroundColor White
