#!/usr/bin/env bash
# Runtime-dependency gate for the Linux desktop .deb (release hardening).
#
# Why: since preview-capture shipped (0.11), the packaged binary hard-links
# libpipewire-0.3 / libspa-0.2 / libgbm / libEGL / libwayland-client. Tauri's
# default deb Depends only cover the GTK/webkit trio, so on systems without
# PipeWire the package installs fine and dies at launch. This gate makes the
# failure impossible to ship:
#
#   1. Static audit (always): print the binary's NEEDED sonames next to the
#      declared Depends, flagging sonames not mentioned directly. Informational
#      only — apt resolves transitively, so "not in Depends" is a hint, not a
#      verdict.
#   2. Container gate (deb + docker): install the .deb in a PRISTINE base
#      image (the distro floor, ubuntu:22.04) with the declared Depends, then
#      `ldd -r` every packaged binary. Any "not found" library or "undefined
#      symbol" line fails the gate — this catches BOTH under-declared deps AND
#      symbol versions newer than the floor's runtime libs (e.g. a pipewire-rs
#      build against newer headers than jammy ships).
#
# Usage:
#   scripts/check-deb-runtime-deps.sh <path.deb> [--distro ubuntu:22.04] [--no-container]
#
# Exit codes: 0 pass/skip, 1 gate failure, 2 usage/environment error.

set -euo pipefail

usage() {
  echo "usage: $0 <path.deb> [--distro IMAGE] [--no-container]" >&2
  exit 2
}

DEB_ARG=""
DISTRO="ubuntu:22.04"
CONTAINER=1
while [ $# -gt 0 ]; do
  case "$1" in
    --distro) DISTRO="${2:?--distro needs a value}"; shift 2 ;;
    --no-container) CONTAINER=0; shift ;;
    -h|--help) usage ;;
    -*) usage ;;
    *) if [ -z "$DEB_ARG" ]; then DEB_ARG="$1"; shift; else usage; fi ;;
  esac
done
[ -n "$DEB_ARG" ] || usage

DEB=$(compgen -G "$DEB_ARG" | head -1 || true)
if [ -z "$DEB" ] || [ ! -f "$DEB" ]; then
  echo "FAIL: .deb not found: $DEB_ARG" >&2
  exit 2
fi
DEB_NAME=$(basename "$DEB")

command -v readelf >/dev/null 2>&1 || { echo "FAIL: binutils (readelf) is required" >&2; exit 2; }
command -v dpkg-deb >/dev/null 2>&1 || { echo "FAIL: dpkg-deb is required" >&2; exit 2; }

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
dpkg-deb -x "$DEB" "$TMP/root"

BIN=$(find "$TMP/root" -type f -name 'shannon-desktop' -perm -u+x | head -1)
[ -n "$BIN" ] || { echo "FAIL: shannon-desktop binary not found in package" >&2; exit 1; }

echo "== $DEB_NAME"
DEPENDS=$(dpkg-deb -f "$DEB" Depends)
echo "declared Depends: $DEPENDS"

echo "== NEEDED sonames (shannon-desktop)"
NEEDED=$(readelf -d "$BIN" | sed -n 's/.*(NEEDED).*\[\(.*\)\].*/\1/p')
[ -n "$NEEDED" ] || { echo "FAIL: no NEEDED entries parsed — readelf output changed?" >&2; exit 1; }
while IFS= read -r soname; do
  [ -n "$soname" ] || continue
  base="${soname%%.so*}"
  # Not in Depends is fine when apt pulls the lib transitively (libc, libdbus,
  # …); the container gate below is the verdict.
  if echo "$DEPENDS" | grep -q "$base"; then
    echo "  $soname  (covered by Depends)"
  else
    echo "  $soname  [not directly in Depends — must resolve transitively]"
  fi
done <<< "$NEEDED"

# The bundled CLI (externalBin) ships in the same package; audit it too when present.
CLI=$(find "$TMP/root" -type f -name 'shannon' -perm -u+x | head -1)
if [ -n "$CLI" ]; then
  echo "== NEEDED sonames (shannon CLI)"
  readelf -d "$CLI" | sed -n 's/.*(NEEDED).*\[\(.*\)\].*/\1/p' | sed 's/^/  /'
fi

if [ "$CONTAINER" -eq 1 ]; then
  if ! command -v docker >/dev/null 2>&1; then
    echo "SKIP: docker unavailable — ran the static audit only (pass --no-container to silence)"
    exit 0
  fi
  echo "== container gate: pristine $DISTRO, declared Depends only, then ldd -r"
  DEB_DIR=$(cd "$(dirname "$DEB")" && pwd)
  # Not `set -e`-guarded on purpose: docker's exit code decides via the if.
  if docker run --rm -v "$DEB_DIR:/pkg:ro" "$DISTRO" bash -uc '
    set -o pipefail
    export DEBIAN_FRONTEND=noninteractive
    apt-get update -qq
    # Default install (recommends ON) mirrors what a real user gets.
    apt-get install -y "/pkg/'"$DEB_NAME"'" > /dev/null
    status=0
    for bin in /usr/bin/shannon-desktop /usr/bin/shannon; do
      [ -x "$bin" ] || continue
      echo "-- $bin"
      unresolved=$(ldd -r "$bin" 2>&1 | grep -E "not found|undefined symbol" || true)
      if [ -n "$unresolved" ]; then
        echo "$unresolved" | sed "s/^/     /"
        echo "     ^^ unresolved runtime dependency or symbol"
        status=1
      else
        echo "   all libraries and symbols resolve"
      fi
    done
    exit $status
  '; then
    echo "container gate: PASS"
  else
    echo "FAIL: container runtime-dependency gate failed (see unresolved list above)" >&2
    exit 1
  fi
fi

echo "PASS: $DEB_NAME"
