#!/usr/bin/env node
// Batch-1 codemod 5/5 (UI review 2026-09-29 §5, task P1): converge page-level
// content widths on the three --container-* tiers added to index.css @theme:
//   narrow  = 48rem  (reading, dialogs)   medium = 75rem (workbench/settings)
//   wide    = 100rem (dashboard)
//
// Mapping (all were page-level containers; 48rem/75rem/100rem keep every
// layout within ±4% of its old value):
//   max-w-3xl      (48rem)  → max-w-narrow
//   max-w-6xl      (72rem)  → max-w-medium
//   max-w-[1000px] (Settings content) → max-w-medium
//   max-w-[1200px]           → max-w-medium
//   max-w-[1600px]           → max-w-wide
//
// Component-level small widths (w-[280px], max-w-[240px] chips, …) are NOT
// touched. The dead --spacing-max-content-width token (zero references) was
// removed from index.css in the same change.
//
// EXECUTED 2026-09-29 on branch fix/ui-review-2026-09-29: 19 replacements
// (7× max-w-3xl, 7× max-w-6xl, 3× max-w-[1200px], 1× max-w-[1600px],
// 1× max-w-[1000px]). Re-run is a no-op.
//
// Usage: node scripts/codemods/migrate-container-widths.mjs [--write]

import { listSrcFiles, finish } from './_lib.mjs'
import { readFileSync, writeFileSync } from 'node:fs'

const dryRun = !process.argv.includes('--write')

const MAP = [
  [/max-w-3xl\b/g, 'max-w-narrow'],
  [/max-w-6xl\b/g, 'max-w-medium'],
  [/max-w-\[1000px\]/g, 'max-w-medium'],
  [/max-w-\[1200px\]/g, 'max-w-medium'],
  [/max-w-\[1600px\]/g, 'max-w-wide'],
]

let changedFiles = 0
const edits = {}

for (const p of listSrcFiles()) {
  const src = readFileSync(p, 'utf8')
  let next = src
  for (const [re, rep] of MAP) {
    next = next.replace(re, m => {
      edits[`${m}→${rep}`] = (edits[`${m}→${rep}`] ?? 0) + 1
      return rep
    })
  }
  if (next !== src) {
    changedFiles++
    if (!dryRun) writeFileSync(p, next)
  }
}

finish({ changedFiles, edits, label: 'page container widths → 3 tiers', dryRun })
