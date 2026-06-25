#!/usr/bin/env bash
# Stop vite + tauri dev processes started by dev-start.sh.
# Falls back to pkill + port-free if PID files are missing.
set -uo pipefail

PID_DIR="${XDG_RUNTIME_DIR:-/tmp}/shannon-dev"
VITE_PID_FILE="$PID_DIR/vite.pid"
TAURI_PID_FILE="$PID_DIR/tauri.pid"
PORT=1420

kill_if_alive() {
  local label="$1" pidfile="$2"
  if [[ -f "$pidfile" ]]; then
    local pid
    pid="$(cat "$pidfile" 2>/dev/null || true)"
    if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
      echo "==> killing $label (pid $pid)"
      kill "$pid" 2>/dev/null || true
    fi
    rm -f "$pidfile"
  fi
}

kill_if_alive tauri "$TAURI_PID_FILE"
kill_if_alive vite  "$VITE_PID_FILE"

# Fallback: catch orphans (started in another shell, lost PID file, etc.)
pkill -f "cargo run" 2>/dev/null && echo "==> killed stray 'cargo run'" || true
pkill -f "shannon-desktop" 2>/dev/null && echo "==> killed stray shannon-desktop binary" || true
pkill -f "pnpm.*ui.*dev" 2>/dev/null && echo "==> killed stray vite (pnpm dev)" || true
pkill -f "vite.*--port 1420" 2>/dev/null && echo "==> killed stray vite (port 1420)" || true

# Final sweep: whoever holds the port.
EXISTING="$(lsof -ti :$PORT 2>/dev/null || true)"
if [[ -n "$EXISTING" ]]; then
  echo "==> port :$PORT still held by pid $EXISTING; SIGTERM"
  kill $EXISTING 2>/dev/null || true
  sleep 1
  EXISTING="$(lsof -ti :$PORT 2>/dev/null || true)"
  if [[ -n "$EXISTING" ]]; then
    echo "==> still alive; SIGKILL"
    kill -9 $EXISTING 2>/dev/null || true
  fi
fi

echo "==> done"
