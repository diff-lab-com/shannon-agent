import { useMemo } from 'react'

interface HtmlRendererProps {
  source: string
  title?: string
}

const STRICT_CSP = '<meta http-equiv="Content-Security-Policy" content="default-src \'none\'; script-src \'unsafe-inline\'; style-src \'unsafe-inline\' \'unsafe-hashes\'; img-src data:; font-src data:;">'

export function HtmlRenderer({ source, title }: HtmlRendererProps) {
  const srcDoc = useMemo(() => {
    if (source.includes('<head>')) {
      return source.replace('<head>', `<head>${STRICT_CSP}`)
    }
    if (source.includes('<html')) {
      return source.replace('<html', `<html><head>${STRICT_CSP}</head>`)
    }
    return `<!DOCTYPE html><html><head>${STRICT_CSP}</head><body>${source}</body></html>`
  }, [source])

  return (
    <iframe
      title={title || 'HTML preview'}
      srcDoc={srcDoc}
      sandbox="allow-scripts"
      loading="lazy"
      className="w-full h-full bg-white border-0"
      style={{ minHeight: '300px' }}
    />
  )
}
