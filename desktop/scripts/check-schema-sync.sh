#!/usr/bin/env bash
# Verify ui/src/schema/events.schema.json matches the canonical schema
# emitted by shannon-types. The sibling checkout at ../shannon-code must
# exist and be at the pinned rev (Cargo.toml [patch] block).
#
# Exits 0 if schemas match, 1 if they differ, 2 if sibling missing.
set -euo pipefail

cd "$(dirname "$0")/.."

ENGINE_DIR="${SHANNON_CODE_DIR:-../shannon-code}"
ENGINE_SCHEMA="$ENGINE_DIR/crates/shannon-types/schema/events.schema.json"
LOCAL_SCHEMA="ui/src/schema/events.schema.json"

if [[ ! -f "$ENGINE_SCHEMA" ]]; then
  echo "WARN: $ENGINE_SCHEMA not found — sibling shannon-code checkout missing."
  echo "      Skipping schema-sync check. Set SHANNON_CODE_DIR to override."
  exit 2
fi

if diff -q "$ENGINE_SCHEMA" "$LOCAL_SCHEMA" >/dev/null; then
  echo "OK: events.schema.json matches shannon-types."
  exit 0
fi

echo "FAIL: ui/src/schema/events.schema.json diverges from shannon-types."
echo "      Rebuild with: cargo build -p shannon-types in ../shannon-code,"
echo "      then copy crates/shannon-types/schema/events.schema.json to ui/src/schema/."
exit 1
