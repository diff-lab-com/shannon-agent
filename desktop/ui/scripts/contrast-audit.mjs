// WCAG contrast audit for the theme token blocks in src/index.css.
//
// Parses every theme's token definitions (the default `@theme` block counts
// as `material`), computes WCAG 2.x contrast ratios for the token pairs the
// UI actually renders text with, and reports pairs that fail AA:
//   4.5:1 for normal-size text, 3:1 for the large-text / UI-component pairs.
//
// Usage: node scripts/contrast-audit.mjs [--min <ratio>]
// Exit code 1 when any required pair fails, so it can gate CI later.

import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const css = readFileSync(join(root, 'src', 'index.css'), 'utf8')

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
for (const m of css.matchAll(/@theme\s*\{([^}]+)\}/g)) {
  themes.material = { ...parseVars(m[1]), ...themes.material }
}
for (const m of css.matchAll(/\[data-theme='([\w-]+)'\]\s*\{([^}]+)\}/g)) {
  themes[m[1]] = parseVars(m[2])
}

// Resolve `var(--x)` references (e.g. --color-link: var(--color-primary))
// against the same block so the audit can check derived tokens.
for (const vars of Object.values(themes)) {
  for (const [name, value] of Object.entries(vars)) {
    const ref = /^var\(--([\w-]+)\)$/.exec(value)
    if (ref && vars[ref[1]]) vars[name] = vars[ref[1]]
  }
}

// ── WCAG math ───────────────────────────────────────────────────────────────
function luminance(hex) {
  const [r, g, b] = [1, 3, 5].map(i => parseInt(hex.slice(i, i + 2), 16) / 255)
  const lin = c => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4)
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}

function contrast(a, b) {
  const [l1, l2] = [luminance(a), luminance(b)].sort((x, y) => y - x)
  return (l1 + 0.05) / (l2 + 0.05)
}

// ── The pairs the UI renders ───────────────────────────────────────────────
// fg/borders on the worst (lightest for dark themes) surface they sit on.
// Ratios: 4.5 = AA normal text, 3.0 = AA large text / UI components.
const PAIRS = [
  ['foreground', 'background', 4.5, 'body text on background'],
  ['foreground', 'surface-container-high', 4.5, 'body text on container surfaces'],
  ['foreground', 'card', 4.5, 'body text on cards'],
  ['foreground', 'popover', 4.5, 'body text in popovers'],
  ['foreground', 'muted', 4.5, 'body text on muted (aria-expanded menus)'],
  ['muted-foreground', 'background', 4.5, 'secondary text on background'],
  ['muted-foreground', 'card', 4.5, 'secondary text on cards'],
  ['muted-foreground', 'surface-container-high', 4.5, 'secondary text on containers'],
  ['muted-foreground', 'muted', 4.5, 'secondary text on muted chips'],
  ['card-foreground', 'card', 4.5, 'card text'],
  ['primary-foreground', 'primary', 4.5, 'primary button label'],
  ['secondary-foreground', 'secondary', 4.5, 'secondary chip label'],
  ['accent-foreground', 'accent', 4.5, 'accent item label'],
  ['color-on-surface', 'color-surface-container-highest', 4.5, 'MD3 text on highest container'],
  ['color-on-surface-variant', 'color-surface-container-high', 4.5, 'MD3 secondary text on containers'],
  ['color-on-primary', 'color-primary', 4.5, 'MD3 primary label'],
  ['color-on-primary-container', 'color-primary-container', 4.5, 'MD3 primary-container label'],
  ['color-link', 'background', 4.5, 'link text on background'],
  ['color-link', 'color-surface-container-lowest', 4.5, 'link text on markdown surfaces'],
  ['outline', 'background', 3.0, 'borders / iconography vs background'],
]

let failures = 0
for (const [theme, vars] of Object.entries(themes)) {
  const rows = []
  for (const [fg, bg, min, label] of PAIRS) {
    if (!vars[fg] || !vars[bg]) continue
    const ratio = contrast(vars[fg], vars[bg])
    if (ratio < min) {
      failures++
      rows.push(`  FAIL ${ratio.toFixed(2)}:1 (min ${min})  ${fg} on ${bg}  — ${label}  [${vars[fg]} on ${vars[bg]}]`)
    }
  }
  if (rows.length) {
    console.log(`\n${theme}:`)
    console.log(rows.join('\n'))
  }
}

console.log(failures === 0 ? '\nAll theme token pairs pass AA.' : `\n${failures} failing pair(s).`)
process.exitCode = failures === 0 ? 0 : 1
