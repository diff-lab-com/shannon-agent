import { useMemo } from 'react'

interface SvgRendererProps {
  source: string
  title?: string
}

export function SvgRenderer({ source, title }: SvgRendererProps) {
  const dataUrl = useMemo(() => {
    const sanitized = source.trim()
    const encoded = encodeURIComponent(sanitized)
    return `data:image/svg+xml;utf8,${encoded}`
  }, [source])

  return (
    <img
      src={dataUrl}
      alt={title || 'SVG diagram'}
      className="w-full h-full object-contain bg-white"
      style={{ minHeight: '200px' }}
    />
  )
}
