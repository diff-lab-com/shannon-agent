#!/usr/bin/env bash
# Facade hygiene gate: banned stale facts must not reappear in user-facing
# surfaces. Each pattern below marks a fact that was wrong/outdated in the
# past and must be removed on sight — extend the list as decisions land
# (see docs/marketing/page-and-docs-improvement-plan-2026-09.md §7).
set -euo pipefail
cd "$(dirname "$0")/.."

pat='7,889|8,600|v0\.1\.0|10-20x|10-20 倍|github\.com/shannon-agent/shannon-code|ericdong/shannon-code'

# shellcheck disable=SC2086
if grep -rnE "$pat" README.md README.zh-CN.md website/src docs-mdbook/src 2>/dev/null; then
  echo ""
  echo "✗ stale/banned phrases found in facade surfaces (see matches above)."
  echo "  Fix the content, or — if the fact is genuinely current — update"
  echo "  scripts/check-facade.sh and record why in the marketing plan doc."
  exit 1
fi
echo "✓ facade surfaces clean (no stale facts)"
