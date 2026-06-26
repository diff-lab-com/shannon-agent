import { useMemo } from 'react'

interface MermaidRendererProps {
  source: string
  title?: string
}

const MERMAID_CDN = 'https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.esm.min.mjs'

const CSP = `default-src 'none'; script-src 'unsafe-inline' ${MERMAID_CDN}; style-src 'unsafe-inline'; font-src data:; img-src data:`

function buildSrcDoc(source: string): string {
  const payload = JSON.stringify(source)
  return [
    '<!DOCTYPE html>',
    '<html>',
    '<head>',
    `<meta http-equiv="Content-Security-Policy" content="${CSP}">`,
    '<style>',
    'html, body { margin: 0; padding: 16px; background: transparent; font-family: system-ui, sans-serif; }',
    '.container { display: flex; align-items: center; justify-content: center; min-height: 100vh; box-sizing: border-box; }',
    '#target { max-width: 100%; overflow: auto; }',
    '.error { color: #ef4444; font-size: 13px; padding: 12px; border: 1px solid #fecaca; background: #fef2f2; border-radius: 6px; }',
    '</style>',
    '</head>',
    '<body>',
    '<div class="container"><div id="target"><div class="error">Rendering diagram…</div></div></div>',
    '<script type="module">',
    'const source = ' + payload + ';',
    `import mermaid from '${MERMAID_CDN}';`,
    'try {',
    "  mermaid.initialize({ startOnLoad: false, theme: 'default', securityLevel: 'strict' });",
    "  const { svg } = await mermaid.render('mmd-preview', source);",
    "  document.getElementById('target').innerHTML = svg;",
    '} catch (err) {',
    "  const msg = (err?.message || String(err)).replace(/[<>&]/g, c => ({ '<':'&lt;','>':'&gt;','&':'&amp;' }[c]));",
    "  document.getElementById('target').innerHTML = '<div class=\"error\">Failed to render: ' + msg + '</div>';",
    '}',
    '<\/script>',
    '</body>',
    '</html>',
  ].join('\n')
}

export function MermaidRenderer({ source, title }: MermaidRendererProps) {
  const srcDoc = useMemo(() => buildSrcDoc(source), [source])
  return (
    <iframe
      title={title || 'Mermaid diagram'}
      srcDoc={srcDoc}
      sandbox="allow-scripts"
      loading="lazy"
      className="w-full h-full bg-background border-0"
      style={{ minHeight: '300px' }}
    />
  )
}
