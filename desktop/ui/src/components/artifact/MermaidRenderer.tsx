import { useEffect, useMemo, useState } from 'react'
import { useIntl } from 'react-intl'
import { themeModeOf, useTheme } from '@/context/ThemeContext'

interface MermaidRendererProps {
  source: string
  title?: string
}

// Review §P2-17: mermaid ships as a bundled npm dependency and is loaded via
// a dynamic import, so it lands in its own lazy chunk (fetched on the first
// diagram) instead of the main bundle. The previous implementation imported
// mermaid from cdn.jsdelivr.net inside the iframe srcDoc — the production
// CSP allows no CDN origins (and srcdoc inherits the parent CSP), so every
// diagram silently failed to render offline. Rendering now happens in the
// parent document and only the resulting SVG is handed to the iframe.

// The srcdoc document receives the *rendered* SVG — static markup, no
// scripts — so its CSP needs no script-src at all (default-src 'none'
// blocks everything else). `sandbox` drops allow-scripts accordingly.
const CSP = `default-src 'none'; style-src 'unsafe-inline'; img-src data:; font-src data:`

// Monotonic counter so concurrent renders never collide on mermaid's
// temporary DOM id (two diagrams can stream in at once).
let renderSeq = 0

export function MermaidRenderer({ source, title }: MermaidRendererProps) {
  const intl = useIntl()
  const { resolvedTheme } = useTheme()
  const loadingLabel = intl.formatMessage({ id: 'artifact.mermaid.loading' })
  const failedLabel = intl.formatMessage({ id: 'artifact.mermaid.renderFailed' })
  const diagramTitle = intl.formatMessage({ id: 'artifact.mermaid.diagramTitle' })
  const mode = themeModeOf(resolvedTheme)
  const [svg, setSvg] = useState<string | null>(null)
  const [failed, setFailed] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    setSvg(null)
    setFailed(null)
    void (async () => {
      try {
        const mermaid = (await import('mermaid')).default
        mermaid.initialize({
          startOnLoad: false,
          securityLevel: 'strict',
          theme: mode === 'dark' ? 'dark' : 'default',
        })
        const { svg: rendered } = await mermaid.render(`mmd-preview-${++renderSeq}`, source)
        if (!cancelled) setSvg(rendered)
      } catch (err) {
        if (!cancelled) setFailed((err as { message?: string })?.message ?? String(err))
      }
    })()
    return () => { cancelled = true }
  }, [source, mode])

  const srcDoc = useMemo(() => (svg ? buildSrcDoc(svg, mode) : ''), [svg, mode])

  return (
    <div className="w-full h-full bg-background" style={{ minHeight: '300px' }}>
      {failed != null ? (
        <div
          role="alert"
          className="m-sm p-sm rounded-lg border border-error/30 bg-error-container/20 text-error text-label-sm break-words"
        >
          <span className="material-symbols-outlined mr-xs align-middle text-[14px]">error</span>
          {failedLabel}
          {failed}
        </div>
      ) : svg == null ? (
        <div
          role="status"
          className="m-sm p-sm rounded-lg border border-outline-variant/20 bg-surface-container text-on-surface-variant text-label-sm"
        >
          {loadingLabel}
        </div>
      ) : (
        <iframe
          title={title || diagramTitle}
          srcDoc={srcDoc}
          // The document is static SVG — no scripts, no same-origin access.
          // An empty sandbox is the strictest form: script execution is
          // impossible even if a diagram ever smuggled markup past mermaid's
          // `securityLevel: 'strict'` sanitizer.
          sandbox=""
          loading="lazy"
          className="w-full h-full border-0"
        />
      )}
    </div>
  )
}

/**
 * Build the iframe document for an already-rendered mermaid SVG.
 *
 * The SVG is inlined as markup (mermaid's own sanitizer ran with
 * `securityLevel: 'strict'` during render). The document runs no scripts —
 * enforced by both the sandbox and a CSP without any script-src — so no
 * script-block escaping is needed; the payload can never execute.
 */
export function buildSrcDoc(svg: string, mode: 'light' | 'dark'): string {
  return [
    '<!DOCTYPE html>',
    `<html data-mode="${mode}">`,
    '<head>',
    `<meta http-equiv="Content-Security-Policy" content="${CSP}">`,
    '<style>',
    'html, body { margin: 0; padding: 16px; background: transparent; font-family: system-ui, sans-serif; }',
    "html[data-mode='dark'] { color-scheme: dark; }",
    '#container svg { max-width: 100%; height: auto; }',
    '</style>',
    '</head>',
    '<body>',
    `<div id="container">${svg}</div>`,
    '</body>',
    '</html>',
  ].join('\n')
}
