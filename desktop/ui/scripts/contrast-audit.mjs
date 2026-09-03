// WCAG contrast audit for the theme token blocks in src/theme/generated/themes.css.
//
// Parses every theme's token definitions (the default `@theme` block counts
// as `material`), computes WCAG 2.x contrast ratios for the token pairs the
// UI actually renders text with, and reports pairs that fail AA:
//   4.5:1 for normal-size text, 3:1 for the large-text / UI-component pairs.
// The pair contract itself lives in scripts/lib/contrast.mjs (shared with
// scripts/generate-themes.mjs, which validates BEFORE emitting).
//
// Usage: node scripts/contrast-audit.mjs [--min <ratio>]
// Exit code 1 when any required pair fails, so it can gate CI.

import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'
import { auditThemes } from './lib/contrast.mjs'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
// Base palette lives in index.css's GENERATED:THEME_BASE region (the @theme
// block must stay in the entry stylesheet — see scripts/generate-themes.mjs);
// per-theme override blocks are the generated themes.css.
const indexCss = readFileSync(join(root, 'src', 'index.css'), 'utf8')
const css = readFileSync(join(root, 'src', 'theme', 'generated', 'themes.css'), 'utf8')

// ── Parse theme blocks ──────────────────────────────────────────────────────
function parseVars(body) {
  const vars = {}
  for (const m of body.matchAll(/--([\w-]+):\s*(#[0-9a-fA-F]{6})\b/g)) {
    vars[m[1]] = m[2]
  }
  return vars
}

const themes = {}
// The default @theme block is the material scheme's source of truth.
for (const m of indexCss.matchAll(/@theme\s*\{([^}]+)\}/g)) {
  themes.material = { ...parseVars(m[1]), ...themes.material }
}
for (const m of css.matchAll(/\[data-theme='([\w-]+)'\]\s*\{([^}]+)\}/g)) {
  themes[m[1]] = parseVars(m[2])
}

const failures = auditThemes(themes)
const byTheme = new Map()
for (const f of failures) {
  const rows = byTheme.get(f.theme) ?? []
  rows.push(`  FAIL ${f.ratio.toFixed(2)}:1 (min ${f.min})  ${f.fg} on ${f.bg}  — ${f.label}  [${f.fgValue} on ${f.bgValue}]`)
  byTheme.set(f.theme, rows)
}
for (const [theme, rows] of byTheme) {
  console.log(`\n${theme}:`)
  console.log(rows.join('\n'))
}

console.log(
  failures.length === 0
    ? '\nAll theme token pairs pass AA.'
    : `\n${failures.length} failing pair(s).`,
)
process.exitCode = failures.length === 0 ? 0 : 1
