#!/usr/bin/env bash
# verify-migration.sh [phase] — check Shannon monorepo migration acceptance gates.
# Run from the monorepo root (the staging clone during migration, or the real repo after).
# Usage:  ./scripts/verify-migration.sh        # all gates
#         ./scripts/verify-migration.sh 2      # just Phase 2
#         ./scripts/verify-migration.sh 3      # just Phase 3 (TS independence)
#         ./scripts/verify-migration.sh A      # just Phase A (protocol)
#
# Reports every failed check within the chosen phase, then exits non-zero if any failed.
set -uo pipefail

phase="${1:-all}"
rc=0

c_ok(){   printf "  \033[32m✓\033[0m %s\n" "$1"; }
c_fail(){ printf "  \033[31m✗\033[0m %s\n" "$1"; rc=1; }
c_sec(){  printf "\n\033[1m[Phase %s]\033[0m\n" "$1"; }

gate_p1(){
  c_sec 1
  if [ -d crates ];  then c_ok "crates/ present";  else c_fail "crates/ missing";  fi
  if [ -d desktop ]; then c_ok "desktop/ present"; else c_fail "desktop/ missing"; fi
  if [ -d gateway ]; then c_ok "gateway/ present"; else c_fail "gateway/ missing"; fi
  if git log --oneline -1 >/dev/null 2>&1; then c_ok "git history present"; else c_fail "no git history"; fi
}

gate_p2(){
  c_sec 2
  if grep -q 'git = "ssh://git@github.com/shannon-agent/shannon-code.git"' desktop/Cargo.toml 2>/dev/null; then
    c_fail "desktop/Cargo.toml still has engine git deps"
  else c_ok "no engine git deps in desktop"; fi
  if grep -q '\[patch' desktop/Cargo.toml 2>/dev/null; then
    c_fail "desktop/Cargo.toml still has a [patch] block"
  else c_ok "no [patch] block in desktop"; fi
  if grep -Eq 'shannon-[a-z-]+[[:space:]]*(\.workspace|=[[:space:]]*\{[[:space:]]*workspace)' desktop/Cargo.toml 2>/dev/null; then
    c_ok "desktop uses workspace deps"
  else c_fail "desktop not on workspace deps"; fi
  if cargo metadata --no-deps --format-version 1 >/dev/null 2>&1; then
    c_ok "cargo metadata resolves"
  else c_fail "cargo metadata fails — workspace not valid"; fi
}

gate_p3(){
  c_sec 3
  # Decision: desktop/ui and gateway are INDEPENDENT TS packages — no pnpm workspace.
  if [ -f desktop/ui/package.json ]; then c_ok "desktop/ui has own package.json"; else c_fail "desktop/ui/package.json missing"; fi
  if [ -f gateway/package.json ];    then c_ok "gateway has own package.json";    else c_fail "gateway/package.json missing"; fi
  if [ ! -f pnpm-workspace.yaml ];   then c_ok "no root pnpm-workspace.yaml (TS packages independent)"; else c_fail "root pnpm-workspace.yaml present — re-couples the products (see Phase 3)"; fi
  if [ -f desktop/ui/pnpm-lock.yaml ]; then c_ok "desktop/ui has own lockfile"; else c_fail "desktop/ui has no pnpm-lock.yaml"; fi
  if [ -f gateway/pnpm-lock.yaml ];    then c_ok "gateway has own lockfile";    else c_fail "gateway has no pnpm-lock.yaml"; fi
}

gate_pa(){
  c_sec A
  if [ -d crates/shannon-api-protocol ]; then c_ok "protocol crate exists"; else c_fail "no crates/shannon-api-protocol"; fi
  if grep -Eq 'interface (WsClientMessage|WsServerMessage|ApprovalRespondRequest)\b' gateway/src/engine/types.ts 2>/dev/null; then
    c_fail "gateway still hand-writes protocol types (run: just gen-protocol)"
  else c_ok "gateway not hand-writing protocol types"; fi
}

case "$phase" in
  1) gate_p1 ;;
  2) gate_p2 ;;
  3) gate_p3 ;;
  A|a) gate_pa ;;
  all) gate_p1; gate_p2; gate_p3; gate_pa ;;
  *) echo "usage: $0 [1|2|3|A|all]" >&2; exit 2 ;;
esac

echo
if [ "$rc" -eq 0 ]; then
  echo "✅ gate passed ($phase)"
else
  echo "❌ gate FAILED ($phase) — see ✗ items above"
fi
exit "$rc"
