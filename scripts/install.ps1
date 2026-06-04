# Shannon Code installer for Windows
# Usage:
#   irm https://cdn.shannon.dev/install.ps1 | iex
#
# Downloads the latest Shannon Code binary and installs it to a
# directory on your PATH.

$ErrorActionPreference = 'Stop'

$CDN_BASE = 'https://cdn.shannon.dev/shannon/latest'
$BINARY = 'shannon-x86_64-windows.exe'
$URL = "$CDN_BASE/$BINARY"

# Determine install directory
$InstallDir = if ($env:USERPROFILE) {
    Join-Path $env:USERPROFILE '.shannon\bin'
} else {
    'C:\shannon\bin'
}

$TargetPath = Join-Path $InstallDir 'shannon.exe'

# Create install directory
if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    Write-Host "[info] Created $InstallDir" -ForegroundColor Cyan
}

# Download
Write-Host "[info] Downloading $BINARY..." -ForegroundColor Cyan
try {
    Invoke-WebRequest -Uri $URL -OutFile $TargetPath -UseBasicParsing
} catch {
    Write-Host "[error] Download failed: $_" -ForegroundColor Red
    exit 1
}

# Verify checksum
$ShaUrl = "$URL.sha256"
try {
    $ShaResponse = Invoke-WebRequest -Uri $ShaUrl -UseBasicParsing
    $ExpectedHash = $ShaResponse.Content.Split(' ')[0].Trim()
    $ActualHash = (Get-FileHash -Path $TargetPath -Algorithm SHA256).Hash.ToLower()
    if ($ActualHash -ne $ExpectedHash.ToLower()) {
        Write-Host "[error] Checksum mismatch!" -ForegroundColor Red
        Remove-Item $TargetPath
        exit 1
    }
    Write-Host "[ok] Checksum verified" -ForegroundColor Green
} catch {
    Write-Host "[info] Checksum not available, skipping verification" -ForegroundColor Yellow
}

# Add to PATH if not already there
$UserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($UserPath -notlike "*$InstallDir*") {
    [Environment]::SetEnvironmentVariable('Path', "$UserPath;$InstallDir", 'User')
    $env:Path = "$env:Path;$InstallDir"
    Write-Host "[ok] Added $InstallDir to user PATH" -ForegroundColor Green
}

Write-Host "[ok] Installed Shannon Code to $TargetPath" -ForegroundColor Green
Write-Host ""
Write-Host "[info] Next: set your API key and run:" -ForegroundColor Cyan
Write-Host "  `$env:SHANNON_API_KEY = 'sk-ant-...'"
Write-Host "  shannon"
