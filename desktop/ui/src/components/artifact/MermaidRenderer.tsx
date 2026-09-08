import { useMemo } from 'react'
import { useIntl } from 'react-intl'
import { themeModeOf, useTheme } from '@/context/ThemeContext'

interface MermaidRendererProps {
  source: string
  title?: string
}

const MERMAID_CDN = 'https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.esm.min.mjs'

const CSP = `default-src 'none'; script-src 'unsafe-inline' ${MERMAID_CDN}; style-src 'unsafe-inline'; font-src data:; img-src data:`

// The srcDoc is a separate document — it cannot read the app's CSS custom
// properties, so the theme mode is injected explicitly: mermaid gets a
// matching built-in theme and the error box gets scheme-aware colors.
function buildSrcDoc(source: string, loadingLabel: string, failedLabel: string, mode: 'light' | 'dark'): string {
  const payload = JSON.stringify(source)
  return [
    '<!DOCTYPE html>',
    `<html data-mode="${mode}">`,
    '<head>',
    `<meta http-equiv="Content-Security-Policy" content="${CSP}">`,
    '<style>',
    'html, body { margin: 0; padding: 16px; background: transparent; font-family: system-ui, sans-serif; }',
    '.container { display: flex; align-items: center; justify-content: center; min-height: 100vh; box-sizing: border-box; }',
    '#target { max-width: 100%; overflow: auto; }',
    '.error { color: #ef4444; font-size: 13px; padding: 12px; border: 1px solid #fecaca; background: #fef2f2; border-radius: 6px; }',
    "html[data-mode='dark'] .error { color: #f87171; border-color: #7f1d1d; background: #450a0a; }",
    '</style>',
    '</head>',
    '<body>',
    `<div class="container"><div id="target"><div class="error">${loadingLabel}</div></div></div>`,
    '<script type="module">',
    'const source = ' + payload + ';',
    `import mermaid from '${MERMAID_CDN}';`,
    'try {',
    `  mermaid.initialize({ startOnLoad: false, theme: '${mode === 'dark' ? 'dark' : 'default'}', securityLevel: 'strict' });`,
    "  const { svg } = await mermaid.render('mmd-preview', source);",
    "  document.getElementById('target').innerHTML = svg;",
    '} catch (err) {',
    "  const msg = (err?.message || String(err)).replace(/[<>&]/g, c => ({ '<':'&lt;','>':'&gt;','&':'&amp;' }[c]));",
    `  document.getElementById('target').innerHTML = '<div class="error">${failedLabel}' + msg + '</div>';`,
    '}',
    '</script>',
    '</body>',
    '</html>',
  ].join('\n')
}

export function MermaidRenderer({ source, title }: MermaidRendererProps) {
  const intl = useIntl()
  const { resolvedTheme } = useTheme()
  const loadingLabel = intl.formatMessage({ id: 'artifact.mermaid.loading' })
  const failedLabel = intl.formatMessage({ id: 'artifact.mermaid.renderFailed' })
  const diagramTitle = intl.formatMessage({ id: 'artifact.mermaid.diagramTitle' })
  const mode = themeModeOf(resolvedTheme)
  const srcDoc = useMemo(
    () => buildSrcDoc(source, loadingLabel, failedLabel, mode),
    [source, loadingLabel, failedLabel, mode],
  )
  return (
    <iframe
      title={title || diagramTitle}
      srcDoc={srcDoc}
      sandbox="allow-scripts"
      loading="lazy"
      className="w-full h-full bg-background border-0"
      style={{ minHeight: '300px' }}
    />
  )
}
