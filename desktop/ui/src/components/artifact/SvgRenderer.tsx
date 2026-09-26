import { useMemo } from 'react'
import { ArtifactZoomBar, useArtifactZoom } from './ArtifactZoomBar'

interface SvgRendererProps {
  source: string
  title?: string
}

/**
 * B3 item 22: shared minimal zoom affordance — steps (and Ctrl+wheel)
 * scale the image via CSS transform; no pan, nothing persisted.
 */
export function SvgRenderer({ source, title }: SvgRendererProps) {
  const { zoom, zoomIn, zoomOut, reset, containerRef } = useArtifactZoom()
  const dataUrl = useMemo(() => {
    const sanitized = source.trim()
    const encoded = encodeURIComponent(sanitized)
    return `data:image/svg+xml;utf8,${encoded}`
  }, [source])

  return (
    <div className="relative w-full h-full min-h-0" style={{ minHeight: '200px' }}>
      <div
        ref={containerRef}
        className="w-full h-full overflow-hidden bg-white rounded-lg flex items-start justify-center"
      >
        <img
          src={dataUrl}
          alt={title || 'SVG diagram'}
          className="max-w-full object-contain"
          style={{ transform: `scale(${zoom})`, transformOrigin: 'top center' }}
        />
      </div>
      <ArtifactZoomBar zoom={zoom} zoomIn={zoomIn} zoomOut={zoomOut} reset={reset} className="absolute top-2 right-2 z-10" />
    </div>
  )
}
