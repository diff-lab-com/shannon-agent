#!/usr/bin/env node
// Batch-1 codemod 1/5 (UI review 2026-09-29 §5, task C1-icon): rewrite hard
// `text-[Npx]` font sizes on Material Symbols elements to the icon-* utility
// scale (12/16/20/24/32/48 px, defined in src/index.css @layer utilities).
//
// Mapping (nearest, round up):  ≤13 icon-xs · 14-17 icon-sm · 18-21 icon-md ·
// 22-27 icon-lg · 28-39 icon-xl · ≥40 icon-2xl.
//
// An element is "icon" when the class string itself contains
// `material-symbols-outlined`, or when it is a className-ish string inside a
// JSX element whose opening tag does. Conditional/template chunks of such
// elements are rewritten too. Sizes fed in through outside variables (e.g.
// the size map in components/ui/loading-state.tsx) are NOT seen here — the
// two known spots were fixed by hand in the same change.
//
// EXECUTED 2026-09-29 on branch fix/ui-review-2026-09-29: 298 replacements
// in 99 files (icon-xs 29 · icon-sm 146 · icon-md 104 · icon-lg 10 ·
// icon-xl 7 · icon-2xl 2), plus 7 hand-fixed icon sizes fed through props
// (Spinner/LoadingState call sites in memory/DreamPanel.tsx,
// migration/MigrationWizard.tsx, ui/loading-state.tsx). material-symbols
// lines with text-[Npx]: 298 → 0. Re-running is a no-op.
//
// Usage: node scripts/codemods/migrate-icon-sizes.mjs [--write]

import { classContainers, jsxElementsContaining, applyEdits, listSrcFiles, relPath, finish } from './_lib.mjs'
import { readFileSync, writeFileSync } from 'node:fs'

const dryRun = !process.argv.includes('--write')

function iconClassFor(n) {
  if (n <= 13) return 'icon-xs'
  if (n <= 17) return 'icon-sm'
  if (n <= 21) return 'icon-md'
  if (n <= 27) return 'icon-lg'
  if (n <= 39) return 'icon-xl'
  return 'icon-2xl'
}

const SIZE_RE = /((?:[a-zA-Z0-9-]+:)*)text-\[(\d+)px\]/g
const MARKER = 'material-symbols-outlined'

let changedFiles = 0
const edits = {}
const leftovers = []

for (const p of listSrcFiles()) {
  const src = readFileSync(p, 'utf8')
  if (!src.includes(MARKER) || !src.includes('text-[')) continue

  const { containers } = classContainers(src)
  const iconElems = jsxElementsContaining(src, MARKER)
  if (iconElems.length === 0) continue

  // Icon-context class strings: mention the marker themselves, or sit inside
  // the opening tag of an icon element (covers conditional/template chunks).
  const iconRanges = containers.filter(c =>
    src.slice(c.start, c.end).includes(MARKER) ||
    iconElems.some(e => c.start < e.end && c.end > e.start),
  )
  if (iconRanges.length === 0) continue

  const fileEdits = []
  for (const r of iconRanges) {
    const chunk = src.slice(r.start, r.end)
    for (const m of chunk.matchAll(SIZE_RE)) {
      const rep = `${m[1]}${iconClassFor(Number(m[2]))}`
      fileEdits.push({ start: r.start + m.index, end: r.start + m.index + m[0].length, replacement: rep })
      const key = `text-[${m[2]}px]→${m[1]}${iconClassFor(Number(m[2]))}`
      edits[key] = (edits[key] ?? 0) + 1
    }
  }
  const next = applyEdits(src, fileEdits)
  const changed = next !== src
  if (changed && !dryRun) writeFileSync(p, next)
  if (changed) changedFiles++

  // Anything still combining the marker with a raw px size on one line is
  // something this codemod could not see (variable-fed sizes etc.).
  const LINE_RE = /text-\[\d+px\]/ // non-global: .test() must stay stateless
  next.split('\n').forEach((line, i) => {
    if (line.includes(MARKER) && LINE_RE.test(line)) leftovers.push(`${relPath(p)}:${i + 1}`)
  })
}

finish({ changedFiles, edits, label: 'icon text-[Npx] → icon-*', dryRun, leftovers })
if (leftovers.length && !dryRun) console.log(`\nNOTE: ${leftovers.length} line(s) still pair material-symbols-outlined with text-[Npx] — fix by hand.`)
