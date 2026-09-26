// ArtifactZoomBar — shared minimal zoom affordance for the visual
// renderers (SVG / mermaid / dock image tab; B3 item 22).
//
// Posture: step buttons scale a CSS transform on the content (no pan,
// nothing persisted — the dock tab re-reads at 100%); Ctrl+wheel steps
// too, via a native non-passive listener because React's synthetic
// wheel handler is passive and could neither preventDefault nor stop
// the dock scrolling underneath.

import { useCallback, useEffect, useRef, useState } from 'react'
import { useT } from '@/i18n'

export const ZOOM_STEP = 0.1
export const ZOOM_MIN = 0.25
export const ZOOM_MAX = 4

export function clampZoom(v: number): number {
  return Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, v))
}

interface ArtifactZoom {
  zoom: number
  zoomIn: () => void
  zoomOut: () => void
  reset: () => void
  /** Attach to the content wrapper to get Ctrl+wheel stepping. */
  containerRef: (el: HTMLDivElement | null) => void
}

export function useArtifactZoom(): ArtifactZoom {
  const [zoom, setZoom] = useState(1)
  const zoomIn = useCallback(() => setZoom(z => clampZoom(Math.round((z + ZOOM_STEP) * 100) / 100)), [])
  const zoomOut = useCallback(() => setZoom(z => clampZoom(Math.round((z - ZOOM_STEP) * 100) / 100)), [])
  const reset = useCallback(() => setZoom(1), [])

  const elRef = useRef<HTMLDivElement | null>(null)
  const containerRef = useCallback((el: HTMLDivElement | null) => {
    elRef.current = el
  }, [])

  useEffect(() => {
    const el = elRef.current
    if (!el) return
    const onWheel = (e: WheelEvent) => {
      if (!e.ctrlKey) return
      e.preventDefault()
      const factor = e.deltaY < 0 ? 1 + ZOOM_STEP : 1 - ZOOM_STEP
      setZoom(z => clampZoom(Math.round(z * factor * 100) / 100))
    }
    el.addEventListener('wheel', onWheel, { passive: false })
    return () => el.removeEventListener('wheel', onWheel)
  }, [])

  return { zoom, zoomIn, zoomOut, reset, containerRef }
}

interface ArtifactZoomBarProps {
  zoom: number
  zoomIn: () => void
  zoomOut: () => void
  reset: () => void
  className?: string
}

const BTN =
  'flex items-center justify-center w-6 h-6 rounded-md text-on-surface-variant hover:bg-surface-container-high hover:text-on-surface focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30'

/** The − / % / + control itself; position it via `className` (usually an
 *  absolute overlay in the renderer's corner). */
export function ArtifactZoomBar({ zoom, zoomIn, zoomOut, reset, className }: ArtifactZoomBarProps) {
  const t = useT()
  return (
    <div
      role="group"
      aria-label={t('chat.dock.zoom.aria')}
      className={
        'flex items-center gap-[2px] px-xs py-[2px] rounded-lg bg-surface-container/90 border border-outline-variant/15 backdrop-blur-sm ' +
        (className ?? '')
      }
    >
      <button type="button" className={BTN} onClick={zoomOut} aria-label={t('chat.dock.zoom.out')} title={t('chat.dock.zoom.out')}>
        <span className="material-symbols-outlined text-[16px]" aria-hidden="true">zoom_out</span>
      </button>
      <button
        type="button"
        className="w-10 h-6 rounded-md font-label-xs tabular-nums text-on-surface-variant hover:bg-surface-container-high hover:text-on-surface focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
        onClick={reset}
        aria-label={t('chat.dock.zoom.reset')}
        title={t('chat.dock.zoom.reset')}
      >
        {Math.round(zoom * 100)}%
      </button>
      <button type="button" className={BTN} onClick={zoomIn} aria-label={t('chat.dock.zoom.in')} title={t('chat.dock.zoom.in')}>
        <span className="material-symbols-outlined text-[16px]" aria-hidden="true">zoom_in</span>
      </button>
    </div>
  )
}
