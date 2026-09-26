import { useMemo } from 'react'

interface HtmlRendererProps {
  source: string
  title?: string
}

// B3 §P1-9 "honest static" posture (decision §5-1, short term): srcdoc
// iframes inherit the parent CSP (`script-src 'self'` in production) and
// multiple policies intersect — an injected meta could never re-grant
// scripts, so `allow-scripts` here only ever produced a *silently static*
// page. The posture is now truthful: no script permission at all (the meta
// drops the dead script-src and keeps style/img allowances), and the dock
// surfaces a hint + the OS escape hatch instead of pretending otherwise.
// True interactive HTML is deferred to the `artifact://` custom protocol.
const STATIC_CSP = '<meta http-equiv="Content-Security-Policy" content="default-src \'none\'; style-src \'unsafe-inline\' \'unsafe-hashes\'; img-src data:; font-src data:;">'

export function HtmlRenderer({ source, title }: HtmlRendererProps) {
  const srcDoc = useMemo(() => {
    // Case-insensitive, first-occurrence injection — the old exact
    // `<head>` match missed `<head id=…>` / `<HEAD>` variants (§P2-24).
    if (/<head[^>]*>/i.test(source)) {
      return source.replace(/<head[^>]*>/i, m => `${m}${STATIC_CSP}`)
    }
    if (/<html[^>]*>/i.test(source)) {
      return source.replace(/<html[^>]*>/i, m => `${m}<head>${STATIC_CSP}</head>`)
    }
    return `<!DOCTYPE html><html><head>${STATIC_CSP}</head><body>${source}</body></html>`
  }, [source])

  return (
    <iframe
      title={title || 'HTML preview'}
      srcDoc={srcDoc}
      // Empty sandbox — strictest form (same posture as MermaidRenderer):
      // opaque origin, and script execution impossible even under a dev
      // CSP that would otherwise allow it.
      sandbox=""
      loading="lazy"
      className="w-full h-full bg-white border-0"
      style={{ minHeight: '300px' }}
    />
  )
}
