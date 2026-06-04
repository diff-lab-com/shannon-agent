#!/bin/sh
# Shannon Code installer — https://github.com/shannon-agent/shannon-code
#
# Usage:
#   curl -fsSL https://cdn.shannon.dev/install.sh | sh
#
# This script detects your OS and architecture, downloads the latest
# Shannon Code binary, verifies its SHA-256 checksum, and installs
# it to /usr/local/bin (or a writable directory on your PATH).

set -e

CDN_BASE="https://cdn.shannon.dev/shannon/latest"

info()  { printf '\033[1m[info]\033[0m  %s\n' "$1"; }
ok()    { printf '\033[32m[ok]\033[0m    %s\n' "$1"; }
err()   { printf '\033[31m[error]\033[0m %s\n' "$1" >&2; exit 1; }

# ── Detect OS and architecture ──────────────────────────────────────────────

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS-$ARCH" in
  Darwin-arm64)           BINARY="shannon-aarch64-macos" ;;
  Darwin-x86_64|Darwin-amd64) BINARY="shannon-x86_64-macos" ;;
  Linux-arm64|Linux-aarch64)  BINARY="shannon-aarch64-linux" ;;
  Linux-x86_64|Linux-amd64)   BINARY="shannon-x86_64-linux" ;;
  *) err "Unsupported platform: $OS $ARCH. Please build from source: https://shannon.dev/docs/building-from-source/" ;;
esac

info "Detected: $OS $ARCH → $BINARY"

# ── Determine install directory ─────────────────────────────────────────────

INSTALL_DIR="/usr/local/bin"
if [ ! -w "$INSTALL_DIR" ] 2>/dev/null; then
  INSTALL_DIR="$HOME/.local/bin"
  mkdir -p "$INSTALL_DIR"
fi

TARGET="$INSTALL_DIR/shannon"

# ── Download ────────────────────────────────────────────────────────────────

URL="$CDN_BASE/$BINARY"
SHA_URL="$URL.sha256"

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

info "Downloading $BINARY..."
curl -fSL --progress-bar "$URL" -o "$TMPDIR/shannon" || err "Download failed"

# ── Verify checksum ─────────────────────────────────────────────────────────

if command -v sha256sum >/dev/null 2>&1; then
  info "Verifying checksum..."
  curl -fSL "$SHA_URL" -o "$TMPDIR/shannon.sha256" 2>/dev/null || true
  if [ -f "$TMPDIR/shannon.sha256" ]; then
    (cd "$TMPDIR" && sha256sum -c shannon.sha256) || err "Checksum mismatch — file may be corrupted"
  else
    info "Checksum not available, skipping verification"
  fi
elif command -v shasum >/dev/null 2>&1; then
  info "Verifying checksum..."
  EXPECTED="$(curl -fSL "$SHA_URL" 2>/dev/null | cut -d' ' -f1)" || true
  if [ -n "$EXPECTED" ]; then
    ACTUAL="$(shasum -a 256 "$TMPDIR/shannon" | cut -d' ' -f1)"
    if [ "$ACTUAL" != "$EXPECTED" ]; then
      err "Checksum mismatch — expected $EXPECTED, got $ACTUAL"
    fi
  fi
fi

# ── Install ─────────────────────────────────────────────────────────────────

chmod +x "$TMPDIR/shannon"
mv "$TMPDIR/shannon" "$TARGET"

ok "Installed Shannon Code to $TARGET"

# ── Verify ──────────────────────────────────────────────────────────────────

if command -v shannon >/dev/null 2>&1; then
  ok "shannon $(shannon --version 2>/dev/null || echo 'installed')"
else
  info "Add $INSTALL_DIR to your PATH:"
  info "  export PATH=\"$INSTALL_DIR:\$PATH\""
fi

printf '\n'
info 'Next step: export SHANNON_API_KEY="sk-ant-..." && shannon'
printf '\n'
