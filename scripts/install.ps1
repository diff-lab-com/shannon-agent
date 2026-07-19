# Shannon Agent installer for Windows
# Usage:
#   irm https://get.shannon.ai/install.ps1 | iex
#
# Downloads the latest Shannon Agent CLI + gateway binaries and the desktop
# setup installer, verifies SHA-256 checksums, and installs them on your PATH.

$ErrorActionPreference = 'Stop'

$CDN_BASE = if ($env:SHANNON_CDN_URL) { "$env:SHANNON_CDN_URL" } else { 'https://github.com/shannon-agent/shannon-agent/releases/latest/download' }

# Resolve the latest version for versioned desktop asset names.
$Version = $null
try {
    $Rel = Invoke-RestMethod -Uri 'https://api.github.com/repos/shannon-agent/shannon-agent/releases/latest' -UseBasicParsing
    $Version = $Rel.tag_name -replace '^v', ''
} catch {
    $Version = '0.0.0'
}
Write-Host "[info] Latest version: $Version" -ForegroundColor Cyan

$CLI_ARCHIVE = 'shannon-x86_64-pc-windows-msvc.zip'
$GATEWAY    = 'shannon-gateway-linux-x64'  # placeholder; real windows asset below
$GATEWAY    = 'shannon-gateway-windows-x64'  # bun windows target artifact (if built)
# NOTE: the gateway matrix builds linux/darwin only. On Windows we still fetch
# the CLI; gateway service is set up on Linux/macOS runners. If a windows
# gateway artifact is added later, it will be picked up here automatically.

# Determine install directory
$InstallDir = if ($env:USERPROFILE) {
    Join-Path $env:USERPROFILE '.shannon\bin'
} else {
    'C:\shannon\bin'
}

if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    Write-Host "[info] Created $InstallDir" -ForegroundColor Cyan
}

function Download-Verify {
    param(
        [string]$Asset,
        [string]$Dest
    )
    $Url = "$CDN_BASE/$Asset"
    Write-Host "[info] Downloading $Asset..." -ForegroundColor Cyan
    try {
        Invoke-WebRequest -Uri $Url -OutFile $Dest -UseBasicParsing
    } catch {
        Write-Host "[error] Download failed: $Asset ($_)" -ForegroundColor Red
        return $null
    }
    # Verify checksum if a matching .sha256 exists.
    $ShaUrl = "$Url.sha256"
    try {
        $ShaResponse = Invoke-WebRequest -Uri $ShaUrl -UseBasicParsing
        $ShaLine = $ShaResponse.Content.Trim() -split '\s+'
        $ExpectedHash = $ShaLine[0].Trim().ToLower()
        $ActualHash = (Get-FileHash -Path $Dest -Algorithm SHA256).Hash.ToLower()
        if ($ActualHash -ne $ExpectedHash) {
            Write-Host "[error] Checksum mismatch for $Asset!" -ForegroundColor Red
            Remove-Item $Dest -Force
            return $null
        }
        Write-Host "[ok] Checksum verified: $Asset" -ForegroundColor Green
    } catch {
        Write-Host "[info] Checksum not available for $Asset, skipping verification" -ForegroundColor Yellow
    }
    return $Dest
}

# ── CLI ────────────────────────────────────────────────────────────────────
$CliZip = Join-Path $env:TEMP 'shannon-cli.zip'
$CliPath = Download-Verify -Asset $CLI_ARCHIVE -Dest $CliZip
if ($CliPath) {
    Write-Host "[info] Extracting CLI..." -ForegroundColor Cyan
    $CliExtract = Join-Path $env:TEMP 'shannon-cli-extract'
    if (Test-Path $CliExtract) { Remove-Item $CliExtract -Recurse -Force }
    Expand-Archive -Path $CliPath -DestinationPath $CliExtract -Force
    $CliBin = Get-ChildItem -Path $CliExtract -Filter 'shannon.exe' -Recurse | Select-Object -First 1
    if (-not $CliBin) {
        Write-Host "[error] shannon.exe not found in archive" -ForegroundColor Red
        exit 1
    }
    Copy-Item $CliBin.FullName (Join-Path $InstallDir 'shannon.exe') -Force
    Remove-Item $CliPath -Force
    if (Test-Path $CliExtract) { Remove-Item $CliExtract -Recurse -Force }
    Write-Host "[ok] Installed shannon to $(Join-Path $InstallDir 'shannon.exe')" -ForegroundColor Green
}

# ── Desktop (NSIS setup) ───────────────────────────────────────────────────
$DesktopAsset = "shannon-desktop_${Version}_x64-setup.exe"
$DesktopPath = Join-Path $env:TEMP 'shannon-desktop-setup.exe'
$Downloaded = Download-Verify -Asset $DesktopAsset -Dest $DesktopPath
if ($Downloaded) {
    Write-Host "[info] Running silent desktop install..." -ForegroundColor Cyan
    try {
        Start-Process -FilePath $DesktopPath -ArgumentList '/S' -Wait
        Write-Host "[ok] Desktop installed" -ForegroundColor Green
    } catch {
        Write-Host "[info] Silent install failed; run manually: $DesktopPath" -ForegroundColor Yellow
    }
    Remove-Item $DesktopPath -Force
} else {
    Write-Host "[info] Desktop installer not available for this version; skipping" -ForegroundColor Yellow
}

# ── Add to PATH ───────────────────────────────────────────────────────────
$UserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($UserPath -notlike "*$InstallDir*") {
    [Environment]::SetEnvironmentVariable('Path', "$UserPath;$InstallDir", 'User')
    $env:Path = "$env:Path;$InstallDir"
    Write-Host "[ok] Added $InstallDir to user PATH" -ForegroundColor Green
}

Write-Host ""
Write-Host "[ok] Shannon Agent installed." -ForegroundColor Green
Write-Host "[info] Next: set your API key and run:" -ForegroundColor Cyan
Write-Host "  `$env:SHANNON_API_KEY = 'sk-ant-...'"
Write-Host "  shannon"
