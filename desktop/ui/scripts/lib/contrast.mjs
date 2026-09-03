// Shared WCAG 2.x contrast math + the token-pair contract used by both
// scripts/contrast-audit.mjs (audits the committed CSS) and
// scripts/generate-themes.mjs (validates the source JSON before emitting).
// Ratios: 4.5 = AA normal text, 3.0 = AA large text / UI components.

export function luminance(hex) {
  const [r, g, b] = [1, 3, 5].map(i => parseInt(hex.slice(i, i + 2), 16) / 255)
  const lin = c => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4)
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}

export function contrast(a, b) {
  const [l1, l2] = [luminance(a), luminance(b)].sort((x, y) => y - x)
  return (l1 + 0.05) / (l2 + 0.05)
}

// The pairs the UI renders — fg/borders on the worst (lightest for dark
// themes) surface they sit on. Keyed by token name WITHOUT the leading
// `--` (the source JSON stores names verbatim: shadcn names are bare,
// MD3 names carry a `color-` prefix).
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
  ['color-on-error-container', 'color-error-container', 4.5, 'MD3 error-container label'],
  ['color-link', 'background', 4.5, 'link text on background'],
  ['color-link', 'color-surface-container-lowest', 4.5, 'link text on markdown surfaces'],
  ['outline', 'background', 3.0, 'borders / iconography vs background'],
]

/**
 * Check a map of theme-name → {token-name → hex} against the PAIRS contract.
 * Tokens referenced as `var(--x)` are resolved within the same theme first.
 * Returns [{ theme, fg, bg, min, label, ratio, fgValue, bgValue }] — empty
 * means every present pair passes.
 */
export function auditThemes(themes) {
  const failures = []
  for (const [theme, rawVars] of Object.entries(themes)) {
    const vars = { ...rawVars }
    for (const [name, value] of Object.entries(vars)) {
      const ref = /^var\(--([\w-]+)\)$/.exec(value)
      if (ref && vars[ref[1]]) vars[name] = vars[ref[1]]
    }
    for (const [fg, bg, min, label] of PAIRS) {
      if (!vars[fg] || !vars[bg]) continue
      const ratio = contrast(vars[fg], vars[bg])
      if (ratio < min) {
        failures.push({
          theme, fg, bg, min, label, ratio,
          fgValue: vars[fg], bgValue: vars[bg],
        })
      }
    }
  }
  return failures
}
