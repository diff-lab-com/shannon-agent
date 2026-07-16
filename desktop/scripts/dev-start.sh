#!/usr/bin/env bash
# Start vite dev server (:1420) + tauri desktop app (cargo run).
# Vite is started first; tauri only launches after :1420 responds.
# Ctrl+C exits both. PID + logs land in $PID_DIR for dev-stop.sh.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

PID_DIR="${XDG_RUNTIME_DIR:-/tmp}/shannon-dev"
mkdir -p "$PID_DIR"
VITE_PID_FILE="$PID_DIR/vite.pid"
TAURI_PID_FILE="$PID_DIR/tauri.pid"
VITE_LOG="$PID_DIR/vite.log"
TAURI_LOG="$PID_DIR/tauri.log"
PORT=1420

cleanup() {
  echo
  echo "==> cleaning up dev processes"
  # shellcheck disable=SC2015 # || true is intentional error suppression under set -e
  [[ -f "$TAURI_PID_FILE" ]] && kill "$(cat "$TAURI_PID_FILE")" 2>/dev/null || true
  # shellcheck disable=SC2015
  [[ -f "$VITE_PID_FILE" ]]  && kill "$(cat "$VITE_PID_FILE")"  2>/dev/null || true
  rm -f "$VITE_PID_FILE" "$TAURI_PID_FILE"
}
trap cleanup EXIT INT TERM

# Refuse to double-start.
if [[ -f "$VITE_PID_FILE" ]] && kill -0 "$(cat "$VITE_PID_FILE")" 2>/dev/null; then
  echo "==> vite already running (pid $(cat "$VITE_PID_FILE")); use scripts/dev-stop.sh first"
  exit 1
fi

# Free port :1420 if a stale process holds it.
EXISTING="$(lsof -ti :$PORT 2>/dev/null || true)"
if [[ -n "$EXISTING" ]]; then
  echo "==> port :$PORT held by pid $EXISTING; killing"
  # shellcheck disable=SC2086 # intentional word splitting: lsof returns newline-separated PIDs
  kill $EXISTING 2>/dev/null || true
  sleep 1
fi

echo "==> starting vite (pnpm --dir ui dev)"
( cd ui && exec pnpm dev ) >"$VITE_LOG" 2>&1 &
VITE_PID=$!
echo "$VITE_PID" >"$VITE_PID_FILE"
echo "    pid=$VITE_PID  log=$VITE_LOG"

echo "==> waiting for http://localhost:$PORT"
for i in $(seq 1 60); do
  if curl -fsS "http://localhost:$PORT" >/dev/null 2>&1; then
    echo "    ready (after ${i}s)"
    break
  fi
  if ! kill -0 "$VITE_PID" 2>/dev/null; then
    echo "    vite died during startup; tail of log:"
    tail -n 30 "$VITE_LOG" >&2 || true
    exit 1
  fi
  sleep 1
  [[ $i -eq 60 ]] && { echo "    timed out after 60s"; exit 1; }
done

echo "==> starting tauri (cargo run)"
cargo run >"$TAURI_LOG" 2>&1 &
TAURI_PID=$!
echo "$TAURI_PID" >"$TAURI_PID_FILE"
echo "    pid=$TAURI_PID  log=$TAURI_LOG"
echo "==> Ctrl+C to stop both"

wait "$TAURI_PID"
