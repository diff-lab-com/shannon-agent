// Shared WCAG 2.x contrast math + the token-pair contract used by both
// scripts/contrast-audit.mjs (audits the committed CSS) and
// scripts/generate-themes.mjs (validates the source JSON before emitting).
// Ratios: 4.5 = AA normal text, 3.0 = AA large text / UI components.

import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join } from 'node:path'
import ts from 'typescript'

export function luminance(hex) {
  const [r, g, b] = [1, 3, 5].map(i => parseInt(hex.slice(i, i + 2), 16) / 255)
  const lin = c => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4)
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}

export function contrast(a, b) {
  const [l1, l2] = [luminance(a), luminance(b)].sort((x, y) => y - x)
  return (l1 + 0.05) / (l2 + 0.05)
}

/** sRGB source-over compositing (browser alpha blending blends the 8-bit
 *  gamma-encoded channel values directly — do NOT linearize first). */
export function composite(fgHex, alpha, bgHex) {
  const f = [1, 3, 5].map(i => parseInt(fgHex.slice(i, i + 2), 16))
  const b = [1, 3, 5].map(i => parseInt(bgHex.slice(i, i + 2), 16))
  return '#' + f
    .map((c, i) => Math.round(c * alpha + b[i] * (1 - alpha)).toString(16).padStart(2, '0'))
    .join('')
}

// The pairs the UI renders — fg/borders on the worst (lightest for dark
// themes) surface they sit on. Keyed by token name WITHOUT the leading
// `--` (the source JSON stores names verbatim: shadcn names are bare,
// MD3 names carry a `color-` prefix; auditThemes normalizes both forms).
export const PAIRS = [
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
  ['color-on-secondary-container', 'color-secondary-container', 4.5, 'MD3 secondary-container label'],
  ['color-on-tertiary-container', 'color-tertiary-container', 4.5, 'MD3 tertiary-container label'],
  ['color-on-error-container', 'color-error-container', 4.5, 'MD3 error-container label'],
  // Solid-error labels (2026-10-03, R5 + chat-testing 裁定): the solid
  // `bg-error` + `text-on-error` family (destructive buttons, stop pill,
  // badges) was the one error template pair outside the gate — nord sat at
  // 3.05:1 on it. Same AA contract as the sibling status hues.
  ['color-on-error', 'color-error', 4.5, 'MD3 error label'],
  // Status hues (success/warning/info) complete the error template — every
  // theme must pass the same four-role AA contract (2026-09 status-tokens PR).
  ['color-on-success', 'color-success', 4.5, 'MD3 success label'],
  ['color-on-success-container', 'color-success-container', 4.5, 'MD3 success-container label'],
  ['color-on-warning', 'color-warning', 4.5, 'MD3 warning label'],
  ['color-on-warning-container', 'color-warning-container', 4.5, 'MD3 warning-container label'],
  ['color-on-info', 'color-info', 4.5, 'MD3 info label'],
  ['color-on-info-container', 'color-info-container', 4.5, 'MD3 info-container label'],
  ['color-link', 'background', 4.5, 'link text on background'],
  ['color-link', 'color-surface-container-lowest', 4.5, 'link text on markdown surfaces'],
  ['outline', 'background', 3.0, 'borders / iconography vs background'],
]

// Accent-tinted chips (the `bg-primary/10` + `text-primary` family, G7
// 2026-09-30): the rendered backdrop is the accent itself alpha-composited
// over the chip's parent surface, so the contract models the composite
// rather than the raw token. Parent = surface-container-low (the chip row
// backdrop most chips sit on). These pairs are usage-conditioned: they are
// only ENFORCED for hue/tint combinations that still occur in component
// sources (see chipCompositesInUse) — the components were migrated to the
// MD3 container role pairs (scripts/codemods/migrate-accent-chips.mjs),
// which are gated by the plain PAIRS above, so a dormant composite entry
// must not fail generation. Reintroducing the pattern re-activates the
// entry for every theme whose composite fails AA.
//   [fg token, tint alpha, backdrop token, min, label]
export const CHIP_COMPOSITES = [
  ['color-primary', 0.05, 'color-surface-container-low', 4.5, 'accent chip label on primary/5 tint'],
  ['color-primary', 0.10, 'color-surface-container-low', 4.5, 'accent chip label on primary/10 tint'],
  ['color-primary', 0.15, 'color-surface-container-low', 4.5, 'accent chip label on primary/15 tint'],
  ['color-primary', 0.20, 'color-surface-container-low', 4.5, 'accent chip label on primary/20 tint'],
  ['color-tertiary', 0.05, 'color-surface-container-low', 4.5, 'tertiary chip label on tertiary/5 tint'],
  ['color-tertiary', 0.10, 'color-surface-container-low', 4.5, 'tertiary chip label on tertiary/10 tint'],
  ['color-tertiary', 0.15, 'color-surface-container-low', 4.5, 'tertiary chip label on tertiary/15 tint'],
  ['color-tertiary', 0.20, 'color-surface-container-low', 4.5, 'tertiary chip label on tertiary/20 tint'],
  ['color-error', 0.05, 'color-surface-container-low', 4.5, 'error text on error/5 tint'],
  ['color-error', 0.10, 'color-surface-container-low', 4.5, 'error chip label on error/10 tint'],
  ['color-error', 0.15, 'color-surface-container-low', 4.5, 'error chip label on error/15 tint'],
  ['color-error', 0.20, 'color-surface-container-low', 4.5, 'error chip label on error/20 tint'],
  ['color-secondary', 0.05, 'color-surface-container-low', 4.5, 'secondary chip label on secondary/5 tint'],
  ['color-secondary', 0.10, 'color-surface-container-low', 4.5, 'secondary chip label on secondary/10 tint'],
  ['color-secondary', 0.15, 'color-surface-container-low', 4.5, 'secondary chip label on secondary/15 tint'],
  ['color-secondary', 0.20, 'color-surface-container-low', 4.5, 'secondary chip label on secondary/20 tint'],
  // Status hues (success/warning/info): the status-tokens PR (2026-09) put
  // `bg-success/10`-style chips on the same footing as the accent family.
  // Like the entries above they are usage-conditioned — the paired call
  // sites were migrated to the container roles, so these stay dormant until
  // a literal pairs `text-success` (…) with a rest-state `bg-success/<n>`.
  ['color-success', 0.05, 'color-surface-container-low', 4.5, 'success text on success/5 tint'],
  ['color-success', 0.10, 'color-surface-container-low', 4.5, 'success chip label on success/10 tint'],
  ['color-success', 0.15, 'color-surface-container-low', 4.5, 'success chip label on success/15 tint'],
  ['color-success', 0.20, 'color-surface-container-low', 4.5, 'success chip label on success/20 tint'],
  ['color-warning', 0.05, 'color-surface-container-low', 4.5, 'warning text on warning/5 tint'],
  ['color-warning', 0.10, 'color-surface-container-low', 4.5, 'warning chip label on warning/10 tint'],
  ['color-warning', 0.15, 'color-surface-container-low', 4.5, 'warning chip label on warning/15 tint'],
  ['color-warning', 0.20, 'color-surface-container-low', 4.5, 'warning chip label on warning/20 tint'],
  ['color-info', 0.05, 'color-surface-container-low', 4.5, 'info text on info/5 tint'],
  ['color-info', 0.10, 'color-surface-container-low', 4.5, 'info chip label on info/10 tint'],
  ['color-info', 0.15, 'color-surface-container-low', 4.5, 'info chip label on info/15 tint'],
  ['color-info', 0.20, 'color-surface-container-low', 4.5, 'info chip label on info/20 tint'],
]

// Files where the pattern is still present on purpose: owned by in-flight
// parallel PRs (SidebarSessions group controls, RoutineTemplatesBrowser
// spacing) — excluded from the in-use scan so the dormant composites stay
// dormant. Keep in sync with scripts/codemods/migrate-accent-chips.mjs.
export const CHIP_PATTERN_SKIP_FILES = [
  'components/SidebarSessions.tsx',
  'components/routines/RoutineTemplatesBrowser.tsx',
]

// A chip pair = `text-<hue>` (any variant prefix) and an UNPREFIXED
// rest-state `bg-<hue>/<n>` co-occurring in ONE string literal (same rule
// as the check-design-tokens guard and migrate-accent-chips codemod;
// hover-only tints never pair, and string-literal granularity keeps
// different ternary branches on one source line from pairing).
// Returns the set of `hue/alpha` strings that gate the CHIP_COMPOSITES entries.
export function chipCompositesInUse(srcDir) {
  const used = new Set()
  const TEXT_RE = hue => new RegExp(`(?:[\\w@/\\[\\].-]+:)?text-${hue}(?![\\w/-])`)
  const BG_RE = hue => new RegExp(`(?<![\\w./:-])bg-${hue}/(\\d+)(?![\\w-])`, 'g')
  const scan = src => {
    const sf = ts.createSourceFile('file.tsx', src, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX)
    const visit = node => {
      // Text of string literals and template quasis only — comments, code
      // and identifiers never pair.
      if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) scanText(src.slice(node.getStart(sf) + 1, node.getEnd() - 1))
      else if (ts.isTemplateExpression(node)) {
        scanText(node.head.text)
        for (const span of node.templateSpans) scanText(span.literal.text)
      }
      ts.forEachChild(node, visit)
    }
    ts.forEachChild(sf, visit)
  }
  const scanText = text => {
    // Keep in sync with check-design-tokens.mjs HUES and the
    // migrate-accent-chips codemod HUES map.
    for (const hue of ['primary', 'secondary', 'tertiary', 'error', 'success', 'warning', 'info']) {
      if (!TEXT_RE(hue).test(text)) continue
      for (const m of text.matchAll(BG_RE(hue))) used.add(`${hue}/${Number(m[1])}`)
    }
  }
  const walk = dir => {
    for (const name of readdirSync(dir)) {
      const p = join(dir, name)
      if (statSync(p).isDirectory()) { walk(p); continue }
      if (!/\.(tsx?|css)$/.test(name)) continue
      const rel = p.slice(srcDir.length + 1)
      if (CHIP_PATTERN_SKIP_FILES.some(s => rel === s || rel.endsWith(s))) continue
      scan(readFileSync(p, 'utf8'))
    }
  }
  walk(srcDir)
  return used
}

/**
 * Check a map of theme-name → {token-name → hex} against the PAIRS +
 * CHIP_COMPOSITES contracts. Token keys may be stored bare or with a
 * leading `--` (the generator passes the source JSON through verbatim,
 * the CSS auditor strips it); both are normalized here — before this
 * normalization the generate-time gate silently matched nothing.
 * Tokens referenced as `var(--x)` are resolved within the same theme first.
 *
 * `chipPatterns` (a Set of `hue/alpha` strings as returned by
 * chipCompositesInUse) restricts the composite checks to the chip patterns
 * the components actually render; pass undefined to enforce every entry.
 *
 * Returns [{ theme, fg, bg, min, label, ratio, fgValue, bgValue }] — empty
 * means every present pair passes.
 */
export function auditThemes(themes, { chipPatterns } = {}) {
  const failures = []
  for (const [theme, rawVars] of Object.entries(themes)) {
    const vars = {}
    for (const [name, value] of Object.entries(rawVars)) vars[name.replace(/^--/, '')] = value
    for (const [name, value] of Object.entries(vars)) {
      const ref = /^var\(--([\w-]+)\)$/.exec(value)
      if (ref && vars[ref[1]]) vars[name] = vars[ref[1]]
    }
    const check = (fg, bgValue, min, label, bgTokenName) => {
      if (!vars[fg] || !bgValue) return
      const ratio = contrast(vars[fg], bgValue)
      if (ratio < min) {
        failures.push({
          theme, fg, bg: bgTokenName, min, label, ratio,
          fgValue: vars[fg], bgValue,
        })
      }
    }
    for (const [fg, bg, min, label] of PAIRS) {
      check(fg, vars[bg], min, label, bg)
    }
    for (const [fg, alpha, over, min, label] of CHIP_COMPOSITES) {
      if (chipPatterns && !chipPatterns.has(`${fg.replace(/^color-/, '')}/${alpha * 100}`)) continue
      if (!vars[fg] || !vars[over]) continue
      check(fg, composite(vars[fg], alpha, vars[over]), min, label, `${over} ← ${fg}/${alpha * 100}%`)
    }
  }
  return failures
}
