#!/usr/bin/env node
// Batch-1 codemod 2/5 (UI review 2026-09-29 §5, task C1-text): rewrite raw
// `text-[Npx]` font sizes to the type-scale roles declared in index.css
// @theme (--text-label-*/--text-body-*/--text-headline-*).
//
// px → role mapping (exact matches unless noted):
//    9 → text-label-2xs  (±1 to the new 10px role)
//   10 → text-label-2xs  (new role — 10px had 59 uses, ≥40 threshold, so it
//                         was promoted to `--text-label-2xs` instead of
//                         forcing those call sites to 11px)
//   11 → text-label-xs   12 → text-label-sm
//   13 → text-label-sm   (tie 12/14 — compact-label side)
//   14 → text-body-sm    (14px is dual-role: body-sm and label-md share it;
//                         body-sm is the safe default — plain text keeps
//                         weight 400; genuine labels already carry their own
//                         font-weight class)
//   15 → text-body-md    (tie 14/16 — body side)
//   16 → text-body-md    18 → text-body-lg    20 → text-headline-sm
//   24 → text-headline-md    32 → text-headline-lg    48 → text-display-lg
//
// Off-scale values with no exact/±1 role (26/28/40/120 px …) are LEFT AS-IS
// and listed below — the design-token guard reports them on the warn channel
// until each gets a deliberate decision.
//
// Containers whose class string still carries `material-symbols-outlined`
// are skipped (icon sizes belong to the icon-* scale — see codemod 1).
//
// EXECUTED 2026-09-29 on branch fix/ui-review-2026-09-29 (after
// migrate-icon-sizes.mjs): 354 replacements in 74 files, plus 2 hand-fixed
// class maps outside any container (ui/badge.tsx SIZE_CLASSES and its
// Badge.test.tsx assertion). Non-test text-[Npx] remaining: 4 deliberate
// off-scale values (guard warn channel). Re-run is a no-op.
//
// Usage: node scripts/codemods/migrate-text-sizes.mjs [--write]

import { classContainers, applyEdits, listSrcFiles, relPath, finish } from './_lib.mjs'
import { readFileSync, writeFileSync } from 'node:fs'

const dryRun = !process.argv.includes('--write')

const TEXT_MAP = {
  9: 'text-label-2xs',
  10: 'text-label-2xs',
  11: 'text-label-xs',
  12: 'text-label-sm',
  13: 'text-label-sm',
  14: 'text-body-sm',
  15: 'text-body-md',
  16: 'text-body-md',
  18: 'text-body-lg',
  20: 'text-headline-sm',
  24: 'text-headline-md',
  32: 'text-headline-lg',
  48: 'text-display-lg',
}

const SIZE_RE = /((?:[a-zA-Z0-9-]+:)*)text-\[(\d+)px\]/g
const MARKER = 'material-symbols-outlined'

let changedFiles = 0
const edits = {}
const leftovers = {}

for (const p of listSrcFiles()) {
  const src = readFileSync(p, 'utf8')
  if (!src.includes('text-[')) continue

  const { containers } = classContainers(src)
  // Defensive: strings that still name the icon font are icon sizing, not body text.
  const ranges = containers.filter(c => !src.slice(c.start, c.end).includes(MARKER))
  if (ranges.length === 0) continue

  const fileEdits = []
  for (const r of ranges) {
    const chunk = src.slice(r.start, r.end)
    for (const m of chunk.matchAll(SIZE_RE)) {
      const role = TEXT_MAP[Number(m[2])]
      if (!role) {
        const key = `${relPath(p)}:text-[${m[2]}px]`
        leftovers[key] = (leftovers[key] ?? 0) + 1
        continue
      }
      const rep = `${m[1]}${role}`
      fileEdits.push({ start: r.start + m.index, end: r.start + m.index + m[0].length, replacement: rep })
      const key = `text-[${m[2]}px]→${m[1]}${role}`
      edits[key] = (edits[key] ?? 0) + 1
    }
  }
  const next = applyEdits(src, fileEdits)
  if (next !== src) {
    changedFiles++
    if (!dryRun) writeFileSync(p, next)
  }
}

finish({ changedFiles, edits, label: 'text-[Npx] → type roles', dryRun })
if (Object.keys(leftovers).length) {
  console.log(`\nOff-scale values kept as-is (guard warn channel):`)
  for (const [k, v] of Object.entries(leftovers).sort()) console.log(`  ${k} ×${v}`)
}
