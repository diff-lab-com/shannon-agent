# Shannon Agent installer for Windows
# Usage:
#   irm https://github.com/diff-lab-com/shannon-agent/releases/latest/download/install.ps1 | iex
#
# Components (comma-separated, default "all"):
#   $env:SHANNON_COMPONENTS = 'cli'   # CLI only
#   $env:SHANNON_COMPONENTS = 'cli,gateway'
#
# Downloads the latest Shannon Agent CLI + gateway binaries and the desktop
# setup installer, verifies SHA-256 checksums (per-asset sidecar or the
# release-wide SHA256SUMS), and installs them on your PATH.

$ErrorActionPreference = 'Stop'

$CDN_BASE = if ($env:SHANNON_CDN_URL) { "$env:SHANNON_CDN_URL" } else { 'https://github.com/diff-lab-com/shannon-agent/releases/latest/download' }
$Repo = 'diff-lab-com/shannon-agent'

# Component selection (parity with install.sh).
$Components = if ($env:SHANNON_COMPONENTS) { "$env:SHANNON_COMPONENTS" } else { 'all' }
function Has-Component {
    param([string]$Name)
    if ($Components -eq 'all') { return $true }
    return (",$Components," -like "*,$Name,*")
}
foreach ($c in ($Components -split ',')) {
    if ($c -notin @('cli', 'gateway', 'desktop')) {
        Write-Host "[error] Invalid SHANNON_COMPONENTS value '$Components' (valid: cli, gateway, desktop, all)" -ForegroundColor Red
        exit 1
    }
}
Write-Host "[info] Components: $Components" -ForegroundColor Cyan

# Resolve the latest version for versioned desktop asset names.
$Version = $null
try {
    $Rel = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -UseBasicParsing
    $Version = $Rel.tag_name -replace '^v', ''
} catch {
    $Version = '0.0.0'
}
Write-Host "[info] Latest version: $Version" -ForegroundColor Cyan

$CLI_ARCHIVE = 'shannon-x86_64-pc-windows-msvc.zip'
$GATEWAY    = 'shannon-gateway-windows-x64.exe'  # built by release.yml gateway matrix

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
    # Verify checksum if a matching .sha256 sidecar exists, else fall back to
    # the release-wide SHA256SUMS manifest (desktop/gateway assets ship
    # without sidecars).
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
        $Verified = $false
        try {
            $Sums = Invoke-WebRequest -Uri "$CDN_BASE/SHA256SUMS" -UseBasicParsing
            $Line = ($Sums.Content -split "`n" |
                Where-Object { $_ -match "\s$([regex]::Escape($Asset))\s*$" } |
                Select-Object -First 1)
            if ($Line) {
                $ExpectedHash = ($Line.Trim() -split '\s+')[0].ToLower()
                $ActualHash = (Get-FileHash -Path $Dest -Algorithm SHA256).Hash.ToLower()
                if ($ActualHash -ne $ExpectedHash) {
                    Write-Host "[error] Checksum mismatch for $Asset!" -ForegroundColor Red
                    Remove-Item $Dest -Force
                    return $null
                }
                Write-Host "[ok] Checksum verified (SHA256SUMS): $Asset" -ForegroundColor Green
                $Verified = $true
            }
        } catch { }
        if (-not $Verified) {
            Write-Host "[info] Checksum not available for $Asset, skipping verification" -ForegroundColor Yellow
        }
    }
    return $Dest
}

# ── CLI ────────────────────────────────────────────────────────────────────
if (Has-Component 'cli') {
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
}

# ── Gateway ───────────────────────────────────────────────────────────────
if (Has-Component 'gateway') {
    $GatewayTmp = Join-Path $env:TEMP 'shannon-gateway.exe'
    $GatewayPath = Download-Verify -Asset $GATEWAY -Dest $GatewayTmp
    if ($GatewayPath) {
        Copy-Item $GatewayPath (Join-Path $InstallDir 'shannon-gateway.exe') -Force
        Remove-Item $GatewayPath -Force
        Write-Host "[ok] Installed shannon-gateway to $(Join-Path $InstallDir 'shannon-gateway.exe')" -ForegroundColor Green
    } else {
        Write-Host "[info] shannon-gateway download failed; skipping (retry later or fetch $GATEWAY from the release page)" -ForegroundColor Yellow
    }
}

# ── Desktop (NSIS setup) ───────────────────────────────────────────────────
if (Has-Component 'desktop') {
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
}

# ── Add to PATH ───────────────────────────────────────────────────────────
$UserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($UserPath -notlike "*$InstallDir*") {
    [Environment]::SetEnvironmentVariable('Path', "$UserPath;$InstallDir", 'User')
    $env:Path = "$env:Path;$InstallDir"
    Write-Host "[ok] Added $InstallDir to user PATH" -ForegroundColor Green
}

Write-Host ""
Write-Host "[ok] Shannon Agent installed. Next steps:" -ForegroundColor Green
if (Has-Component 'cli') {
    Write-Host "[info]   1. `$env:SHANNON_API_KEY = 'sk-ant-...'" -ForegroundColor Cyan
    Write-Host "[info]   2. shannon                                          # launch the REPL" -ForegroundColor Cyan
}
if (Has-Component 'gateway') {
    Write-Host "[info]   3. shannon gateway setup                            # initialize ~/.shannon/gateway/config.json" -ForegroundColor Cyan
    Write-Host "[info]   4. shannon gateway install                          # register gateway as background service" -ForegroundColor Cyan
    Write-Host "[info]   5. shannon gateway enroll <platform>                # enroll a chat-platform bot token" -ForegroundColor Cyan
    Write-Host "[info]   Note: the gateway connects to a running engine — keep 'shannon serve' running (e.g. via schtasks) or chat commands will have nothing to talk to." -ForegroundColor Cyan
    Write-Host "[info]   Docs: https://shannon.ai/docs/gateway                # Slack/Telegram/Discord/Matrix/WhatsApp/WeCom/Feishu/DingTalk" -ForegroundColor Cyan
}
