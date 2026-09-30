#!/usr/bin/env node
// G7 codemod (light-theme contrast fix, 2026-09-30): migrate "accent text on
// a translucent accent background" chips (`bg-primary/10 text-primary`, …)
// to the MD3 container role pairs (`bg-primary-container` +
// `text-on-primary-container`). The tint composites are AA-failing on most
// non-default themes (see scripts/lib/contrast.mjs CHIP_COMPOSITES — the
// generate-themes gate now models them), while every on-X-container /
// X-container pair passes 4.5:1 in all 12 themes by construction.
//
// Pairing rule (deliberately conservative): within ONE string literal, a
// hue's `text-<hue>` (any variant prefix) and an UNPREFIXED rest-state
// `bg-<hue>/<alpha>` must BOTH appear for that literal to be rewritten.
// A hover-/focus-only tint (`text-primary hover:bg-primary/10` ghost
// buttons) does NOT pair: migrating it would strand `text-on-*-container`
// on a neutral surface at rest — invisible in themes whose on-container
// token is light (e.g. material). Standalone `text-primary` (links, titles,
// icons) and decorative tints are untouched. Preserved variant prefixes are
// kept on every rewritten class. That pairing rule is itself the safety
// gate, so the scan covers every string literal, not just className/cn(...)
// arguments: the tone maps several components pass into cn() (STATUS_TONES,
// TONE_ACCENT, …) are plain object literals the className/cva detection
// cannot see through.
//
// EXECUTED 2026-09-30 on branch fix/light-theme-accent-contrast (run stats in
// the PR description). Idempotent: after a run no literal pairs the rule, so
// re-running is a no-op.
//
// EXECUTED AGAIN 2026-09 for the status-semantic-tokens PR: success/warning/
// info joined HUES once those hues became real tokens; the four pair literals
// that existed were migrated to their container pairs in the same PR.
//
// Skipped on purpose (parallel PRs own these files — migrate there later):
//   src/components/SidebarSessions.tsx, src/components/routines/RoutineTemplatesBrowser.tsx
//
// Usage: node scripts/codemods/migrate-accent-chips.mjs [--write]

import ts from 'typescript'
import { applyEdits, listSrcFiles, finish, SRC } from './_lib.mjs'
import { readFileSync, writeFileSync } from 'node:fs'
import { relative } from 'node:path'

const dryRun = !process.argv.includes('--write')

// Files owned by in-flight parallel PRs — chip migrations there would collide.
const SKIP = [
  'components/SidebarSessions.tsx',
  'components/routines/RoutineTemplatesBrowser.tsx',
]

// hue → MD3 container role pair
const HUES = {
  primary: { bg: 'bg-primary-container', text: 'text-on-primary-container' },
  secondary: { bg: 'bg-secondary-container', text: 'text-on-secondary-container' },
  tertiary: { bg: 'bg-tertiary-container', text: 'text-on-tertiary-container' },
  error: { bg: 'bg-error-container', text: 'text-on-error-container' },
  // Status hues (2026-09 status-tokens PR): same pairing rule, same roles.
  // Re-runs after that PR migrate any `text-success` + `bg-success/<n>`-style
  // chips introduced since (keep in sync with scripts/lib/contrast.mjs
  // chipCompositesInUse and check-design-tokens.mjs HUES).
  success: { bg: 'bg-success-container', text: 'text-on-success-container' },
  warning: { bg: 'bg-warning-container', text: 'text-on-warning-container' },
  info: { bg: 'bg-info-container', text: 'text-on-info-container' },
}

// Variant prefix (hover:, group-hover/card:, data-[highlighted]:, md:, …).
const PREFIX = '((?:[\\w@/\\[\\].-]+:)?)'
// text-<hue> but NOT text-<hue>-foreground / -fixed / text-<hue>/80.
const textRe = hue => new RegExp(`${PREFIX}text-${hue}(?![\\w/-])`, 'g')
// UNPREFIXED bg-<hue>/<alpha> — a rest-state tinted chip background. The
// `(?<![\w./:-])` lookbehind rejects variant-prefixed occurrences
// (hover:bg-primary/10 — the `:` matters). Replacement is prefix-free by
// construction; `!bg-primary/10` keeps its bang (outside the match).
const bgRe = hue => new RegExp(`(?<![\\w./:-])bg-${hue}/(\\d+)(?![\\w-])`, 'g')

/** Every string-literal / template-quasi text range in the file. */
function literalRanges(src) {
  const sf = ts.createSourceFile('file.tsx', src, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX)
  const out = []
  function visit(node) {
    if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) {
      // +1/-1 trims the quotes so replacements keep them.
      out.push({ start: node.getStart(sf) + 1, end: node.getEnd() - 1 })
    } else if (ts.isTemplateExpression(node)) {
      const push = n => out.push({ start: n.getStart(sf) + 1, end: n.getEnd() - 1 })
      push(node.head)
      for (const span of node.templateSpans) push(span.literal)
    }
    ts.forEachChild(node, visit)
  }
  ts.forEachChild(sf, visit)
  return out
}

let changedFiles = 0
const edits = {}
let skippedFiles = 0

for (const p of listSrcFiles()) {
  const rel = relative(SRC, p)
  if (SKIP.some(s => rel === s || rel.endsWith(s))) { skippedFiles++; continue }
  const src = readFileSync(p, 'utf8')
  if (!/text-(primary|secondary|tertiary|error|success|warning|info)\b/.test(src) && !/bg-(primary|secondary|tertiary|error|success|warning|info)\/\d/.test(src)) continue

  const fileEdits = []
  for (const lit of literalRanges(src)) {
    const text = src.slice(lit.start, lit.end)
    const hueReplacements = []
    for (const [hue, roles] of Object.entries(HUES)) {
      const hasText = textRe(hue).test(text)
      const hasBg = bgRe(hue).test(text)
      if (!hasText || !hasBg) continue
      // Rewrite every rest-state tinted bg (prefix-free by definition) and
      // every accent text of this hue in the literal, preserving each
      // text class's own variant prefix.
      for (const m of text.matchAll(bgRe(hue))) {
        hueReplacements.push({ start: m.index, end: m.index + m[0].length, replacement: roles.bg, from: m[0] })
      }
      for (const m of text.matchAll(textRe(hue))) {
        hueReplacements.push({ start: m.index, end: m.index + m[0].length, replacement: `${m[1]}${roles.text}`, from: m[0] })
      }
    }
    if (hueReplacements.length === 0) continue
    // Non-overlapping by construction (bg/text patterns are disjoint).
    for (const r of hueReplacements) {
      fileEdits.push({ start: lit.start + r.start, end: lit.start + r.end, replacement: r.replacement })
      const to = r.replacement.replace(/^[\w@/[\].-]+:/, '')
      edits[`${r.from}→${to}`] = (edits[`${r.from}→${to}`] ?? 0) + 1
    }
  }
  if (fileEdits.length === 0) continue
  const next = applyEdits(src, fileEdits)
  if (next !== src) {
    changedFiles++
    if (!dryRun) writeFileSync(p, next)
  }
}

finish({
  changedFiles,
  edits,
  label: 'accent chip (text-<hue> + bg-<hue>/<n>) → MD3 container pair',
  dryRun,
  leftovers: skippedFiles ? [`${skippedFiles} file(s) skipped — owned by parallel PRs: ${SKIP.join(', ')}`] : [],
})
