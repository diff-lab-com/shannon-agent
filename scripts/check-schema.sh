#!/usr/bin/env bash
# Check that the committed JSON Schema matches freshly-generated schema.
# This prevents commits where the schema is out of sync with the types.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "Checking JSON Schema consistency..."
cd "$REPO_ROOT"

# Clean build to ensure fresh schema generation
cargo clean -p shannon-types

# Build to generate schema
cargo build -p shannon-types > /dev/null 2>&1

# Compare generated schema with committed schema
if ! diff -q "$REPO_ROOT/crates/shannon-types/schema/events.schema.json" \
           "$REPO_ROOT/target/debug/build/shannon-types-"*/out/events.schema.json > /dev/null 2>&1; then
    echo "❌ Schema mismatch detected!"
    echo "Generated schema does not match committed schema."
    echo "Run: cargo build -p shannon-types"
    echo "Then: cp target/debug/build/shannon-types-*/out/events.schema.json crates/shannon-types/schema/events.schema.json"
    exit 1
fi

echo "✅ Schema check passed"
exit 0
