#!/usr/bin/env node
// Batch-1 codemod 6/6 (UI review 2026-09-29 §5, task T1 follow-up): rewrite
// numeric `duration-<ms>` classes to the @theme duration tokens via v4 CSS
// variable shorthand. --duration-* (100/160/240/400 ms) are the only duration
// definitions in the app since Batch 0.
//
// Nearest-token mapping (as adopted by the design-token guard):
//   100        → duration-(--duration-fast)
//   150 – 200  → duration-(--duration-normal)
//   240 – 300  → duration-(--duration-slow)
//   > 300      → duration-(--duration-slower)
//
// EXECUTED 2026-09-29 on branch fix/ui-review-2026-09-29: 26 replacements
// (dialog/modal/select/side-panel enter-exit 100ms → fast; hover/transition
// 150-200ms → normal; panel/dock 300ms → slow; page enter 500-700ms →
// slower). Re-run is a no-op.
//
// Usage: node scripts/codemods/migrate-durations.mjs [--write]

import { classContainers, applyEdits, listSrcFiles, finish } from './_lib.mjs'
import { readFileSync, writeFileSync } from 'node:fs'

const dryRun = !process.argv.includes('--write')

function durationVarFor(ms) {
  if (ms <= 100) return 'duration-(--duration-fast)'
  if (ms <= 200) return 'duration-(--duration-normal)'
  if (ms <= 300) return 'duration-(--duration-slow)'
  return 'duration-(--duration-slower)'
}

const DURATION_RE = /\bduration-(\d+)\b/g

let changedFiles = 0
const edits = {}

for (const p of listSrcFiles()) {
  const src = readFileSync(p, 'utf8')
  if (!/duration-\d/.test(src)) continue

  const { containers } = classContainers(src)
  if (containers.length === 0) continue

  const fileEdits = []
  for (const r of containers) {
    const chunk = src.slice(r.start, r.end)
    for (const m of chunk.matchAll(DURATION_RE)) {
      const rep = durationVarFor(Number(m[1]))
      fileEdits.push({ start: r.start + m.index, end: r.start + m.index + m[0].length, replacement: rep })
      const key = `${m[0]}→${rep}`
      edits[key] = (edits[key] ?? 0) + 1
    }
  }
  const next = applyEdits(src, fileEdits)
  if (next !== src) {
    changedFiles++
    if (!dryRun) writeFileSync(p, next)
  }
}

finish({ changedFiles, edits, label: 'duration-<ms> → duration-(--duration-*)', dryRun })
