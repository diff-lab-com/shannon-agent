#!/usr/bin/env node
// Batch-1 codemod 4/5 (UI review 2026-09-29 §5, task C3): rewrite numeric
// spacing utilities to the semantic --spacing-* scale (xs 4 / sm 8 / md 16 /
// lg 24 / xl 32 px), and the bare `rounded` shorthand to `rounded-sm`.
//
// Exact-value mapping only (12px has no token — *-3 is left alone):
//   *-1 → *-xs   *-2 → *-sm   *-4 → *-md   *-6 → *-lg   *-8 → *-xl
// for gap / p / px / py / pt / pb / pl / pr / m / mx / my / mt / mb / ml / mr.
// Variant prefixes (hover:p-2 → hover:p-sm) and negative values (-mt-1 →
// -mt-xs) are preserved. Fractional (`p-2.5`) and larger steps (`p-10`) are
// untouched.
//
// `rounded` → `rounded-sm` is exact-word only; every rounded-* suffix class
// (rounded-full, rounded-md, rounded-none, …) is left alone.
//
// EXECUTED 2026-09-29 on branch fix/ui-review-2026-09-29: 503 replacements
// in 99 files (385 spacing: gap-xs 44, gap-sm 48, mb-xs 40, py-xs 35,
// px-sm 28, p-sm 27, … · 118 bare rounded → rounded-sm). 12px steps (*-3),
// fractional steps (*-1.5/*-0.5) and larger steps (*-10+) are untouched by
// design. Re-run is a no-op.
//
// Usage: node scripts/codemods/migrate-spacing-rounded.mjs [--write]

import { classContainers, applyEdits, listSrcFiles, finish } from './_lib.mjs'
import { readFileSync, writeFileSync } from 'node:fs'

const dryRun = !process.argv.includes('--write')

const SUFFIX = { 1: 'xs', 2: 'sm', 4: 'md', 6: 'lg', 8: 'xl' }
const PROPS = ['gap', 'p', 'px', 'py', 'pt', 'pb', 'pl', 'pr', 'm', 'mx', 'my', 'mt', 'mb', 'ml', 'mr']
// (?![\w.-]) keeps p-2.5 / p-20 / data-* lookalikes out.
const SPACING_RE = new RegExp(`\\b(${PROPS.join('|')})-([12468])(?![\\w.-])`, 'g')
// Exact-word `rounded`: no suffix, no preceding word char/dash.
const ROUNDED_RE = /(?<![\w-])rounded(?![\w-])/g

let changedFiles = 0
const edits = {}

for (const p of listSrcFiles()) {
  const src = readFileSync(p, 'utf8')

  const { containers } = classContainers(src)
  if (containers.length === 0) continue

  const fileEdits = []
  for (const r of containers) {
    const chunk = src.slice(r.start, r.end)
    for (const m of chunk.matchAll(SPACING_RE)) {
      const rep = `${m[1]}-${SUFFIX[m[2]]}`
      fileEdits.push({ start: r.start + m.index, end: r.start + m.index + m[0].length, replacement: rep })
      const key = `${m[1]}-${m[2]}→${rep}`
      edits[key] = (edits[key] ?? 0) + 1
    }
    for (const m of chunk.matchAll(ROUNDED_RE)) {
      fileEdits.push({ start: r.start + m.index, end: r.start + m.index + m[0].length, replacement: 'rounded-sm' })
      edits['rounded→rounded-sm'] = (edits['rounded→rounded-sm'] ?? 0) + 1
    }
  }
  const next = applyEdits(src, fileEdits)
  if (next !== src) {
    changedFiles++
    if (!dryRun) writeFileSync(p, next)
  }
}

finish({ changedFiles, edits, label: 'spacing */rounded → semantic tokens', dryRun })
