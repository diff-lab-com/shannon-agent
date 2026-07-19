#!/bin/sh
# Shannon Agent installer — https://github.com/shannon-agent/shannon-agent
#
# Usage:
#   curl -fsSL ${SHANNON_CDN_URL:-https://get.shannon.ai/install.sh} | sh
#
# This script detects your OS and architecture, downloads the latest Shannon
# Agent binaries (CLI + gateway) and desktop bundle, verifies their SHA-256
# checksums, and installs them. CLI and gateway go to /usr/local/bin
# (fallback ~/.local/bin). The desktop app is installed per-OS.

set -e

# R2 mirror (default) falls back to the GitHub release "latest" download.
CDN_BASE="${SHANNON_CDN_URL:-https://github.com/shannon-agent/shannon-agent/releases/latest/download}"

info()  { printf '\033[1m[info]\033[0m  %s\n' "$1"; }
ok()    { printf '\033[32m[ok]\033[0m    %s\n' "$1"; }
err()   { printf '\033[31m[error]\033[0m %s\n' "$1" >&2; exit 1; }

# ── Detect OS and architecture ──────────────────────────────────────────────

OS="$(uname -s)"
ARCH="$(uname -m)"

# Map to the asset triples cargo-dist / tauri-action / bun produce.
case "$OS-$ARCH" in
  Linux-x86_64|Linux-amd64)     CLI="shannon-x86_64-unknown-linux-gnu.tar.gz"
                                GATEWAY="shannon-gateway-linux-x64" ;;
  Linux-aarch64|Linux-arm64)    CLI="shannon-aarch64-unknown-linux-gnu.tar.gz"
                                GATEWAY="shannon-gateway-linux-arm64" ;;
  Darwin-x86_64|Darwin-amd64)  CLI="shannon-x86_64-apple-darwin.tar.gz"
                                GATEWAY="shannon-gateway-darwin-x64" ;;
  Darwin-arm64)                 CLI="shannon-aarch64-apple-darwin.tar.gz"
                                GATEWAY="shannon-gateway-darwin-arm64" ;;
  *) err "Unsupported platform: $OS $ARCH. Please build from source." ;;
esac

# Desktop bundle per OS/arch (Tauri productName = shannon-desktop).
# The %VERSION% token is filled in once we know the resolved version.
DESKTOP=""

info "Detected: $OS $ARCH"

# ── Determine install directory ─────────────────────────────────────────────

INSTALL_DIR="/usr/local/bin"
if [ ! -w "$INSTALL_DIR" ] 2>/dev/null; then
  INSTALL_DIR="$HOME/.local/bin"
  mkdir -p "$INSTALL_DIR"
fi

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

# ── Resolve the latest version (for versioned desktop asset names) ──────────
# Prefer the GitHub API; fall back to scraping the redirect from the
# "latest" download URL. If both fail we leave VERSION empty and skip the
# versioned desktop asset (the CLI/gateway archives are version-independent).

VERSION=""
if command -v curl >/dev/null 2>&1; then
  VERSION="$(curl -fSsL "https://api.github.com/repos/shannon-agent/shannon-agent/releases/latest" 2>/dev/null \
    | grep -m1 '"tag_name"' | sed -E 's/.*"tag_name": *"v?([^"]+)".*/\1/')"
  if [ -z "$VERSION" ]; then
    VERSION="$(curl -fSsL -o /dev/null -w '%{url_effective}' "${CDN_BASE}/../tag/latest" 2>/dev/null \
      | sed -E 's#.*/tag/v?##')"
  fi
fi
[ -n "$VERSION" ] || VERSION=""
info "Latest version: ${VERSION:-unknown}"

# Build the desktop asset name now that we know the version.
case "$OS-$ARCH" in
  Linux-x86_64|Linux-amd64)      DESKTOP="shannon-desktop_${VERSION}_amd64.deb" ;;
  Linux-aarch64|Linux-arm64)     DESKTOP="shannon-desktop_${VERSION}_aarch64.deb" ;;
  Darwin-x86_64|Darwin-amd64)    DESKTOP="shannon-desktop_${VERSION}_x64.dmg" ;;
  Darwin-arm64)                  DESKTOP="shannon-desktop_${VERSION}_aarch64.dmg" ;;
esac

download_verify() {
  # $1 = asset filename (also the URL basename). Verifies a matching .sha256 if
  # present, then prints the local path on success.
  url="$CDN_BASE/$1"
  sha_url="$CDN_BASE/$1.sha256"
  dst="$TMPDIR/$1"
  info "Downloading $1..."
  curl -fSL --progress-bar "$url" -o "$dst" || err "Download failed: $1"

  if command -v sha256sum >/dev/null 2>&1; then
    if curl -fSL "$sha_url" -o "$dst.sha256" 2>/dev/null && [ -s "$dst.sha256" ]; then
      info "Verifying checksum..."
      # cargo-dist sha256 files are named after the asset; rewrite to local file.
      sed "s#.*#$dst#" "$dst.sha256" > "$dst.sha256.local"
      (cd "$TMPDIR" && sha256sum -c "$(basename "$dst").sha256.local") || err "Checksum mismatch: $1"
    else
      info "Checksum not available for $1, skipping verification"
    fi
  fi
  printf '%s' "$dst"
}

# ── Install gateway ────────────────────────────────────────────────────────

GW_PATH="$INSTALL_DIR/shannon-gateway"
GW_FILE="$(download_verify "$GATEWAY")"
cp "$GW_FILE" "$GW_PATH"
chmod +x "$GW_PATH"
ok "Installed shannon-gateway to $GW_PATH"

# ── Install CLI ────────────────────────────────────────────────────────────

CLI_ARCHIVE="$(download_verify "$CLI")"
info "Extracting CLI..."
tar xzf "$CLI_ARCHIVE" -C "$TMPDIR"
CLI_BIN="$(find "$TMPDIR" -name shannon -type f 2>/dev/null | head -1)"
[ -n "$CLI_BIN" ] && [ -f "$CLI_BIN" ] || err "CLI binary not found in archive"
chmod +x "$CLI_BIN"
mv "$CLI_BIN" "$INSTALL_DIR/shannon"
ok "Installed shannon to $INSTALL_DIR/shannon"

# ── Sanity check ───────────────────────────────────────────────────────────

if command -v shannon >/dev/null 2>&1; then
  ok "$(shannon --version 2>/dev/null || echo 'shannon installed')"
else
  info "Add $INSTALL_DIR to your PATH:"
  info "  export PATH=\"$INSTALL_DIR:\$PATH\""
fi

# ── Desktop bundle ─────────────────────────────────────────────────────────

install_desktop() {
  case "$OS" in
    Linux)
      DEB="$(download_verify "shannon-desktop_${VERSION}_amd64.deb" 2>/dev/null || true)"
      RPM="$(download_verify "shannon-desktop-${VERSION}-1.x86_64.rpm" 2>/dev/null || true)"
      if [ -n "$DEB" ] && [ -f "$DEB" ]; then
        info "Installing .deb (requires sudo)..."
        sudo dpkg -i "$DEB" || info ".deb install failed — run: sudo dpkg -i $DEB"
      elif [ -n "$RPM" ] && [ -f "$RPM" ]; then
        info "Installing .rpm (requires sudo)..."
        sudo dnf install -y "$RPM" 2>/dev/null || sudo yum install -y "$RPM" 2>/dev/null \
          || info ".rpm install failed — run: sudo dnf install -y $RPM"
      else
        info "No Linux desktop package matched; skipping desktop install"
      fi
      ;;
    Darwin)
      DMG="$(download_verify "$DESKTOP" 2>/dev/null || true)"
      if [ -n "$DMG" ] && [ -f "$DMG" ]; then
        info "Mounting $DESKTOP..."
        VOL="$(hdiutil attach "$DMG" | grep -oE '/Volumes/[^ ]+' | head -1)"
        if [ -n "$VOL" ]; then
          APP="$(find "$VOL" -maxdepth 2 -name '*.app' | head -1)"
          if [ -n "$APP" ]; then
            cp -R "$APP" /Applications/ && ok "Installed $(basename "$APP") to /Applications"
          fi
          hdiutil detach "$VOL" >/dev/null 2>&1 || true
        fi
      else
        info "No macOS desktop disk image matched; skipping desktop install"
      fi
      ;;
    *)
      info "Automatic desktop install is not supported on $OS; download the bundle from the release page."
      ;;
  esac
}

if [ -n "$DESKTOP" ] && [ -n "$VERSION" ]; then
  install_desktop
elif [ -n "$DESKTOP" ]; then
  info "Could not resolve a version; skipping desktop install (download the bundle manually)."
fi

printf '\n'
ok "Shannon Agent installed. Next steps:"
info "  export SHANNON_API_KEY=\"sk-ant-...\""
info "  shannon                       # launch the REPL"
info "  shannon gateway install       # register the gateway as a background service"
printf '\n'
