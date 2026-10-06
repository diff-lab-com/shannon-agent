import { useEffect, useRef, useState } from 'react'
import { convertFileSrc } from '@tauri-apps/api/core'
import * as pdfjsLib from 'pdfjs-dist'
import type { PDFDocumentProxy, RenderTask } from 'pdfjs-dist'
import workerUrl from 'pdfjs-dist/build/pdf.worker.min.mjs?url'

import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Icon } from '@/components/ui/icon'
import { useT } from '@/i18n'
import { toastError } from '@/lib/errorToast'
import { openWithDefaultApp } from '@/lib/tauri-api'

/**
 * PdfPreview — office Wave 1.5 (B8b): inline PDF preview for .pdf FileCards.
 *
 * Data path mirrors the Wave 1 image lightbox (MessageBubble AttachmentPreview):
 * `convertFileSrc(path)` yields an asset-protocol URL the webview can fetch,
 * pdf.js then parses the bytes and rasterizes the current page onto a canvas.
 * The pdf.js worker is wired the Vite-recommended way — the `?url` import makes
 * Vite emit `pdf.worker.min.mjs` (matching the installed pdfjs-dist v4 build
 * layout) as a hashed asset and hands its URL to `GlobalWorkerOptions.workerSrc`,
 * no bundler plugin needed.
 *
 * Guardrails: browsing is capped at MAX_PREVIEW_PAGES — the indicator shows the
 * capped window and an "open externally" escape hatch appears past it; any
 * load/render failure falls back to `previewFailed` + the same escape hatch
 * (openWithDefaultApp), so a PDF this component cannot render is still one
 * click away from the OS viewer.
 */

// Must stay in sync with the pdfjs-dist build file the `?url` import above
// resolves (v4.x ships build/pdf.worker.min.mjs).
pdfjsLib.GlobalWorkerOptions.workerSrc = workerUrl

/** Page-navigation cap — beyond this the OS viewer takes over. */
const MAX_PREVIEW_PAGES = 200
/** Base raster scale before devicePixelRatio is applied. */
const BASE_SCALE = 1.5

export interface PdfPreviewProps {
  /** Absolute path of the PDF — converted to an asset URL for pdf.js. */
  path: string
  /** Display name shown as the dialog title. */
  name: string
  /** Close request from the X button / Escape / backdrop. */
  onClose: () => void
}

type LoadState = 'loading' | 'ready' | 'error'

export function PdfPreview({ path, name, onClose }: PdfPreviewProps) {
  const t = useT()
  const [state, setState] = useState<LoadState>('loading')
  const [numPages, setNumPages] = useState(0)
  const [page, setPage] = useState(1)
  const docRef = useRef<PDFDocumentProxy | null>(null)
  const canvasRef = useRef<HTMLCanvasElement | null>(null)

  const truncated = numPages > MAX_PREVIEW_PAGES
  // The indicator and the navigation clamp share the capped window — the
  // escape hatch below is how the reader reaches the rest.
  const displayTotal = Math.min(numPages, MAX_PREVIEW_PAGES)

  // Load + parse on mount (per path). `convertFileSrc` gives the same
  // webview-accessible URL the image lightbox uses; fetching the full byte
  // buffer avoids depending on asset-protocol range-request support.
  useEffect(() => {
    let cancelled = false
    let task: ReturnType<typeof pdfjsLib.getDocument> | null = null
    const load = async () => {
      try {
        const res = await fetch(convertFileSrc(path))
        if (!res.ok) throw new Error(`asset fetch failed: HTTP ${res.status}`)
        const data = new Uint8Array(await res.arrayBuffer())
        task = pdfjsLib.getDocument({ data })
        const doc = await task.promise
        if (cancelled) return
        if (doc.numPages < 1) throw new Error('document has no pages')
        docRef.current = doc
        setNumPages(doc.numPages)
        setPage(1)
        setState('ready')
      } catch {
        // Loading a dead / unreadable asset lands here — the failed panel
        // keeps the open-externally escape hatch usable.
        if (!cancelled) setState('error')
      }
    }
    void load()
    return () => {
      cancelled = true
      docRef.current = null
      // destroy() is safe both mid-load and after resolution.
      void task?.destroy()
    }
  }, [path])

  // Rasterize the current page whenever it (or readiness) changes. The canvas
  // keeps its previous frame while the next page renders, so flipping reads
  // as an in-place update instead of a flash of empty canvas.
  useEffect(() => {
    if (state !== 'ready') return
    const doc = docRef.current
    const canvas = canvasRef.current
    if (!doc || !canvas) return
    let cancelled = false
    let renderTask: RenderTask | null = null
    const render = async () => {
      try {
        const pdfPage = await doc.getPage(page)
        if (cancelled || canvasRef.current !== canvas) return
        const dpr = window.devicePixelRatio || 1
        const viewport = pdfPage.getViewport({ scale: BASE_SCALE * dpr })
        canvas.width = Math.floor(viewport.width)
        canvas.height = Math.floor(viewport.height)
        canvas.style.width = `${Math.floor(viewport.width / dpr)}px`
        canvas.style.height = `${Math.floor(viewport.height / dpr)}px`
        const ctx = canvas.getContext('2d')
        if (!ctx) return
        renderTask = pdfPage.render({ canvasContext: ctx, viewport })
        await renderTask.promise
      } catch {
        // A cancelled in-flight render (page flipped mid-raster) rejects with
        // RenderingCancelledException — that is control flow, not failure.
        if (!cancelled) setState('error')
      }
    }
    void render()
    return () => {
      cancelled = true
      renderTask?.cancel()
    }
  }, [state, page])

  const handleOpenExternally = () => {
    openWithDefaultApp(path).catch((err) => toastError(t('link.open.failed'), err))
  }

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent
        data-testid="pdf-preview"
        className="w-[42rem] sm:max-w-[42rem] max-w-[calc(100%-2rem)]"
      >
        <DialogHeader>
          <DialogTitle className="truncate pr-lg" title={name}>{name}</DialogTitle>
          <DialogDescription className="sr-only">{path}</DialogDescription>
        </DialogHeader>

        {state === 'loading' && (
          <div className="flex items-center justify-center py-xl" data-testid="pdf-preview-loading">
            <Icon name="progress_activity" size="lg" className="animate-spin text-on-surface-variant" />
            <span className="sr-only">{t('chat.message.filecard.preview')}</span>
          </div>
        )}

        {state === 'error' && (
          <div
            className="flex flex-col items-center gap-sm py-xl text-center"
            data-testid="pdf-preview-failed"
          >
            <Icon name="error" size="lg" className="text-error" />
            <p className="font-body-sm text-on-surface-variant">
              {t('chat.message.filecard.previewFailed')}
            </p>
            <Button variant="outline" size="sm" onClick={handleOpenExternally}>
              <Icon name="open_in_new" />
              {t('chat.message.attachment.openExternally')}
            </Button>
          </div>
        )}

        {state === 'ready' && (
          <>
            <div className="flex justify-center overflow-auto rounded-lg bg-surface-container-lowest max-h-[70vh]">
              <canvas ref={canvasRef} data-testid="pdf-canvas" className="max-w-full" />
            </div>
            <div className="flex items-center justify-between gap-sm">
              <div className="flex items-center gap-xs">
                <Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label={t('ui.pagination.previous')}
                  disabled={page <= 1}
                  onClick={() => setPage((p) => Math.max(1, p - 1))}
                  className="text-on-surface-variant hover:text-primary hover:bg-surface-container"
                >
                  <Icon name="chevron_left" />
                </Button>
                <span
                  className="font-label-sm text-on-surface-variant tabular-nums px-xs"
                  data-testid="pdf-page-indicator"
                  aria-live="polite"
                >
                  {t('chat.message.filecard.page', { current: page, total: displayTotal })}
                </span>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label={t('ui.pagination.next')}
                  disabled={page >= displayTotal}
                  onClick={() => setPage((p) => Math.min(displayTotal, p + 1))}
                  className="text-on-surface-variant hover:text-primary hover:bg-surface-container"
                >
                  <Icon name="chevron_right" />
                </Button>
              </div>
              {truncated && (
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={handleOpenExternally}
                  className="gap-xs text-on-surface-variant hover:text-primary"
                >
                  <Icon name="open_in_new" />
                  {t('chat.message.attachment.openExternally')}
                </Button>
              )}
            </div>
          </>
        )}
      </DialogContent>
    </Dialog>
  )
}
