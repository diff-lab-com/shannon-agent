#!/bin/sh
# Shannon Code installer — https://github.com/shannon-agent/shannon-code
#
# Usage:
#   curl -fsSL ${SHANNON_CDN_URL:-https://github.com/shannon-agent/shannon-code/releases/latest/download}/install.sh | sh
#
# This script detects your OS and architecture, downloads the latest
# Shannon Code binary, verifies its SHA-256 checksum, and installs
# it to /usr/local/bin (or a writable directory on your PATH).

set -e

CDN_BASE="${SHANNON_CDN_URL:-https://github.com/shannon-agent/shannon-code/releases/latest/download}"

info()  { printf '\033[1m[info]\033[0m  %s\n' "$1"; }
ok()    { printf '\033[32m[ok]\033[0m    %s\n' "$1"; }
err()   { printf '\033[31m[error]\033[0m %s\n' "$1" >&2; exit 1; }

# ── Detect OS and architecture ──────────────────────────────────────────────

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS-$ARCH" in
  Darwin-arm64)               ARCHIVE="shannon-cli-aarch64-apple-darwin.tar.gz" ;;
  Darwin-x86_64|Darwin-amd64) ARCHIVE="shannon-cli-x86_64-apple-darwin.tar.gz" ;;
  Linux-arm64|Linux-aarch64)  ARCHIVE="shannon-cli-aarch64-unknown-linux-musl.tar.gz" ;;
  Linux-x86_64|Linux-amd64)   ARCHIVE="shannon-cli-x86_64-unknown-linux-musl.tar.gz" ;;
  *) err "Unsupported platform: $OS $ARCH. Please build from source." ;;
esac

info "Detected: $OS $ARCH"

# ── Determine install directory ─────────────────────────────────────────────

INSTALL_DIR="/usr/local/bin"
if [ ! -w "$INSTALL_DIR" ] 2>/dev/null; then
  INSTALL_DIR="$HOME/.local/bin"
  mkdir -p "$INSTALL_DIR"
fi

TARGET="$INSTALL_DIR/shannon"

# ── Download ────────────────────────────────────────────────────────────────

URL="$CDN_BASE/$ARCHIVE"
SHA_URL="$URL.sha256"

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

info "Downloading $ARCHIVE..."
curl -fSL --progress-bar "$URL" -o "$TMPDIR/$ARCHIVE" || err "Download failed"

# ── Verify checksum ─────────────────────────────────────────────────────────

if command -v sha256sum >/dev/null 2>&1; then
  info "Verifying checksum..."
  curl -fSL "$SHA_URL" -o "$TMPDIR/$ARCHIVE.sha256" 2>/dev/null || true
  if [ -f "$TMPDIR/$ARCHIVE.sha256" ]; then
    (cd "$TMPDIR" && sha256sum -c "$ARCHIVE.sha256") || err "Checksum mismatch"
  else
    info "Checksum not available, skipping verification"
  fi
fi

# ── Extract and install ─────────────────────────────────────────────────────

info "Extracting..."
tar xzf "$TMPDIR/$ARCHIVE" -C "$TMPDIR"

BINARY="$(find "$TMPDIR" -name shannon -type f 2>/dev/null | head -1)"
if [ -z "$BINARY" ]; then
  # cargo-dist puts binary in a subdirectory named after the archive
  DIRNAME="${ARCHIVE%.tar.gz}"
  BINARY="$TMPDIR/$DIRNAME/shannon"
fi

[ -f "$BINARY" ] || err "Binary not found in archive"

chmod +x "$BINARY"
mv "$BINARY" "$TARGET"

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
