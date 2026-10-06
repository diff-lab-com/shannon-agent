#!/usr/bin/env bash
# preflight-network.sh — probe the three network dependencies an eval sweep
# needs, before burning hours into a batch that dies on egress flaps.
#
# Evidence (docs/backlog.md §一): two acceptance sweeps were invalidated by
# network-window flakiness — 18 first-call connection failures to the model
# API, and 12–19 verifier verdicts lost to uv-bootstrap download errors.
#
# Probes:
#   model-api    — the LLM endpoint (must be reachable or agents cannot run)
#   github       — uv/toolchain downloads inside verifier containers
#   uv-cdn       — astral.sh + the uv release asset on objects.githubusercontent
#
# Usage:
#   preflight-network.sh [--model-url URL] [--min-healthy N] [--quiet]
#
# Exit codes: 0 = at least --min-healthy probes passed; 1 = fewer.
set -uo pipefail

MODEL_URL="${MODEL_URL:-https://open.bigmodel.cn/api/coding/paas/v4}"
GITHUB_URL="${GITHUB_URL:-https://github.com}"
UV_URL="${UV_URL:-https://astral.sh/uv/0.7.13/install.sh}"
UV_ASSET_URL="${UV_ASSET_URL:-https://github.com/astral-sh/uv/releases/download/0.9.5/uv-x86_64-unknown-linux-gnu.tar.gz}"
MIN_HEALTHY=3
TIMEOUT=15
QUIET=0
while [ $# -gt 0 ]; do
  case "$1" in
    --model-url) MODEL_URL="${2:?}"; shift 2 ;;
    --min-healthy) MIN_HEALTHY="${2:?}"; shift 2 ;;
    --quiet) QUIET=1; shift ;;
    *) echo "unknown arg: $1" >&2; exit 1 ;;
  esac
done

probe() { # name url [head-only]
  local name="$1" url="$2" code
  code=$(timeout "$TIMEOUT" curl -s -o /dev/null \
    -w '%{http_code} %{time_total}' -L "$url" 2>/dev/null | cut -d' ' -f1)
  # Any HTTP response (even 401/404) proves TCP+TLS+HTTP egress works.
  case "$code" in
    000) echo "UNREACHABLE"; return 1 ;;
    *) echo "OK($code)"; return 0 ;;
  esac
}

healthy=0
for spec in "model-api:$MODEL_URL" "github:$GITHUB_URL" "uv-cdn:$UV_URL" "uv-asset:$UV_ASSET_URL"; do
  name="${spec%%:*}"; url="${spec#*:}"
  r=$(probe "$name" "$url"); rc=$?
  [ "$QUIET" != 1 ] && echo "  $name: $r"
  [ "$rc" -eq 0 ] && healthy=$((healthy + 1))
done

echo "healthy: $healthy/4 (min $MIN_HEALTHY)"
[ "$healthy" -ge "$MIN_HEALTHY" ]
