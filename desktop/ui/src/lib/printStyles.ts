// Shared print/export stylesheet builder.
//
// Print windows open in a separate document that doesn't inherit our
// theme CSS variables. To get correct on-surface / surface-container
// colors in the exported HTML, we resolve the tokens from the parent
// document at injection time via getComputedStyle and emit literal values.

type Variant = 'chat' | 'report'

function resolveToken(name: string, fallback: string): string {
  if (typeof window === 'undefined') return fallback
  const root = document.documentElement
  const value = getComputedStyle(root).getPropertyValue(name).trim()
  return value || fallback
}

interface Options {
  variant: Variant
}

export function buildPrintStyles({ variant }: Options): string {
  const onSurface = resolveToken('--color-on-surface', '#1a1a1a')
  const onSurfaceVariant = resolveToken('--color-on-surface-variant', '#555')
  const outlineVariant = resolveToken('--color-outline-variant', '#ddd')
  const surfaceContainerLow = resolveToken('--color-surface-container-low', '#f5f5f5')
  const secondary = resolveToken('--color-secondary', '#0066cc')

  const base = `
    body { font: 14px/1.6 -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; padding: 32px; color: ${onSurface}; max-width: 760px; margin: 0 auto; }
    pre { background: ${surfaceContainerLow}; padding: 12px; border-radius: 6px; overflow-x: auto; }
    code { font-family: ui-monospace, 'SF Mono', Menlo, monospace; font-size: 13px; }
    p { white-space: pre-wrap; }
    strong { font-weight: 600; }
  `

  if (variant === 'chat') {
    return base + `
      h1 { font-size: 22px; margin-bottom: 4px; }
      h3 { font-size: 14px; margin-top: 24px; color: ${onSurfaceVariant}; text-transform: uppercase; letter-spacing: 0.04em; }
      hr { border: 0; border-top: 1px solid ${outlineVariant}; margin: 16px 0; }
    `
  }

  return base + `
    h1 { font-size: 24px; margin: 0 0 4px; }
    h2 { font-size: 15px; margin-top: 28px; color: ${onSurfaceVariant}; text-transform: uppercase; letter-spacing: 0.04em; border-bottom: 1px solid ${outlineVariant}; padding-bottom: 4px; }
    h3 { font-size: 16px; margin-top: 22px; }
    .summary { font-style: italic; color: ${onSurfaceVariant}; margin: 12px 0 24px; }
    .meta { color: ${onSurfaceVariant}; font-size: 12px; margin-bottom: 24px; }
    .citations { margin-top: 32px; padding-top: 16px; border-top: 1px solid ${outlineVariant}; }
    .citation { margin-bottom: 8px; padding-left: 28px; position: relative; }
    .citation .num { position: absolute; left: 0; top: 0; font-weight: 600; color: ${onSurfaceVariant}; }
    .citation a { color: ${secondary}; text-decoration: none; word-break: break-all; }
    .citation a:hover { text-decoration: underline; }
  `
}
