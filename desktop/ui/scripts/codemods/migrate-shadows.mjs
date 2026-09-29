#!/usr/bin/env node
// Batch-1 codemod 3/5 (UI review 2026-09-29 §5, task T2/C3): rewrite raw
// Tailwind elevation classes to the shadow-eN scale (@theme --shadow-e1..5,
// bound by the shadow-eN @utility in index.css):
//   shadow-sm → shadow-e1 (card rest)      shadow-md → shadow-e2 (card hover)
//   shadow-lg → shadow-e3 (dropdown)       shadow-xl → shadow-e4 (drawer)
//   shadow-2xl → shadow-e5 (modal)
//
// Exception (kept raw, listed here): a class string that ALSO carries a
// shadow COLOR companion (shadow-primary/30, shadow-error/30, …). Colored
// shadows rely on Tailwind's --tw-shadow-color variable mechanism, which the
// fixed-value shadow-eN box-shadow cannot compose. The design-token guard
// warns (not errors) on those lines.
//
// shadow-none / shadow-inner / shadow-[arbitrary] are out of scope.
//
// EXECUTED 2026-09-29 on branch fix/ui-review-2026-09-29: 145 replacements,
// 5 color-companion lines kept (Sidebar ×2, Header, MicButton, ChatInput —
// see git history). Re-run is a no-op.
//
// Usage: node scripts/codemods/migrate-shadows.mjs [--write]

import { classContainers, applyEdits, listSrcFiles, relPath, finish } from './_lib.mjs'
import { readFileSync, writeFileSync } from 'node:fs'

const dryRun = !process.argv.includes('--write')

const SHADOW_MAP = { sm: 'shadow-e1', md: 'shadow-e2', lg: 'shadow-e3', xl: 'shadow-e4', '2xl': 'shadow-e5' }
const NON_SIZE = new Set(['none', 'inner', ...Object.keys(SHADOW_MAP), 'e1', 'e2', 'e3', 'e4', 'e5'])

// shadow-<token>, optionally variant-prefixed (hover:shadow-lg) or followed
// by an opacity modifier (shadow-primary/30) or arbitrary value (shadow-[…]).
const SHADOW_TOKEN_RE = /((?:[a-zA-Z0-9-]+:)*)shadow-([^\s"'`,)]+)/g

let changedFiles = 0
const edits = {}
const keptColorCompanions = []

for (const p of listSrcFiles()) {
  const src = readFileSync(p, 'utf8')
  if (!/shadow-/.test(src)) continue

  const { containers } = classContainers(src)
  if (containers.length === 0) continue

  const fileEdits = []
  for (const r of containers) {
    const chunk = src.slice(r.start, r.end)
    // Analyze per quoted string, not per container: a cn() call can mix a
    // colored-shadow branch (skip) with clean branches (migrate).
    const spans = []
    for (const q of chunk.matchAll(/(["'])(?:\\.|(?!\1)[^\\])*\1/g)) spans.push({ start: r.start + q.index, text: q[0] })
    if (spans.length === 0) spans.push({ start: r.start, text: chunk })
    for (const span of spans) {
      const sizes = []
      let hasColorCompanion = false
      for (const m of span.text.matchAll(SHADOW_TOKEN_RE)) {
        const token = m[2].replace(/\/.*$/, '') // strip /opacity
        if (Object.hasOwn(SHADOW_MAP, token)) sizes.push(m)
        else if (!NON_SIZE.has(token)) hasColorCompanion = true
      }
      if (sizes.length === 0) continue
      if (hasColorCompanion) {
        const lineNo = src.slice(0, span.start).split('\n').length
        keptColorCompanions.push(`${relPath(p)}:${lineNo}`)
        continue
      }
      for (const m of sizes) {
        const rep = `${m[1]}${SHADOW_MAP[m[2]]}`
        fileEdits.push({ start: span.start + m.index, end: span.start + m.index + m[0].length, replacement: rep })
        const key = `shadow-${m[2]}→${m[1]}${SHADOW_MAP[m[2]]}`
        edits[key] = (edits[key] ?? 0) + 1
      }
    }
  }
  const next = applyEdits(src, fileEdits)
  if (next !== src) {
    changedFiles++
    if (!dryRun) writeFileSync(p, next)
  }
}

finish({ changedFiles, edits, label: 'shadow-{sm..2xl} → shadow-e{1..5}', dryRun })
if (keptColorCompanions.length) {
  console.log(`\nKept raw (shadow-color companion present — guard warn channel): ${keptColorCompanions.length}`)
  for (const l of [...new Set(keptColorCompanions)]) console.log(`  ${l}`)
}
