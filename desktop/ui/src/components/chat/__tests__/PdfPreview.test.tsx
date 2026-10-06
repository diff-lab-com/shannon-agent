import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'

import { openWithDefaultApp } from '@/lib/tauri-api'
import { PdfPreview } from '../PdfPreview'

// Per-file pdfjs-dist mock (hoisted factory, same pattern as FileCard.test):
// the component only touches `getDocument` and `GlobalWorkerOptions` — the
// fake doc exposes numPages/getPage/destroy and each fake page a viewport +
// a render() that resolves immediately.
const { getDocument, GlobalWorkerOptions } = vi.hoisted(() => ({
  getDocument: vi.fn(),
  GlobalWorkerOptions: { workerSrc: '' },
}))

vi.mock('pdfjs-dist', () => ({ getDocument, GlobalWorkerOptions }))

// convertFileSrc stays on the global setup mock (asset://localhost/…), so the
// fetch stub below asserts the full converted URL the component loads from.
const fetchMock = vi.fn()

function makeFakePage() {
  return {
    getViewport: vi.fn(({ scale }: { scale: number }) => ({
      width: 100 * scale,
      height: 140 * scale,
    })),
    render: vi.fn(() => ({ promise: Promise.resolve(), cancel: vi.fn() })),
  }
}

function makeFakeDoc(numPages: number) {
  const pages = new Map<number, ReturnType<typeof makeFakePage>>()
  const doc = {
    numPages,
    getPage: vi.fn(async (n: number) => {
      if (!pages.has(n)) pages.set(n, makeFakePage())
      return pages.get(n)!
    }),
    destroy: vi.fn(async () => {}),
  }
  return doc
}

function stubFetchOk() {
  fetchMock.mockResolvedValue({
    ok: true,
    status: 200,
    arrayBuffer: async () => new ArrayBuffer(8),
  })
}

const PROPS = { path: '/tmp/shannon/report.pdf', name: 'report.pdf', onClose: vi.fn() }

// Restored per-test so the setup file's shared Element.prototype mocks stay
// intact for the rest of the file (a blanket restoreAllMocks would reset
// getAnimations to undefined and crash base-ui's unmount path).
let getContextSpy: ReturnType<typeof vi.spyOn>

beforeEach(() => {
  vi.clearAllMocks()
  vi.mocked(openWithDefaultApp).mockResolvedValue(undefined)
  stubFetchOk()
  vi.stubGlobal('fetch', fetchMock)
  // jsdom has no canvas implementation — hand back a stub 2d context so the
  // component's raster path reaches the (mocked) pdf.js render() call.
  getContextSpy = vi
    .spyOn(HTMLCanvasElement.prototype, 'getContext')
    .mockReturnValue({} as unknown as CanvasRenderingContext2D)
})

afterEach(() => {
  cleanup()
  getContextSpy.mockRestore()
  vi.unstubAllGlobals()
})

describe('PdfPreview', () => {
  it('fetches the asset URL and shows page 1 of N once the document parses', async () => {
    const doc = makeFakeDoc(3)
    getDocument.mockReturnValue({ promise: Promise.resolve(doc), destroy: vi.fn(async () => {}) })

    render(<PdfPreview {...PROPS} />)

    // convertFileSrc (global setup mock) → asset://localhost/<path sans lead />
    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('asset://localhost/tmp/shannon/report.pdf')
    })
    await waitFor(() => {
      expect(getDocument).toHaveBeenCalledWith({ data: expect.any(Uint8Array) })
    })
    expect(await screen.findByText('Page 1 / 3')).toBeInTheDocument()
    expect(screen.getByTestId('pdf-canvas')).toBeInTheDocument()
    await waitFor(() => {
      expect(doc.getPage).toHaveBeenCalledWith(1)
    })
  })

  it('flips to the next page and back via the pagination buttons', async () => {
    const doc = makeFakeDoc(3)
    getDocument.mockReturnValue({ promise: Promise.resolve(doc), destroy: vi.fn(async () => {}) })

    render(<PdfPreview {...PROPS} />)
    await screen.findByText('Page 1 / 3')

    const prev = screen.getByRole('button', { name: 'Previous page' })
    const next = screen.getByRole('button', { name: 'Next page' })
    expect(prev).toBeDisabled()
    expect(next).toBeEnabled()

    fireEvent.click(next)
    expect(await screen.findByText('Page 2 / 3')).toBeInTheDocument()
    await waitFor(() => {
      expect(doc.getPage).toHaveBeenCalledWith(2)
    })
    expect(prev).toBeEnabled()

    fireEvent.click(prev)
    expect(await screen.findByText('Page 1 / 3')).toBeInTheDocument()
  })

  it('shows the failed panel with an open-externally escape hatch when loading breaks', async () => {
    fetchMock.mockResolvedValue({ ok: false, status: 404, arrayBuffer: async () => new ArrayBuffer(0) })

    render(<PdfPreview {...PROPS} />)

    expect(await screen.findByTestId('pdf-preview-failed')).toBeInTheDocument()
    expect(screen.getByText('Preview failed')).toBeInTheDocument()
    expect(screen.queryByTestId('pdf-canvas')).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Open externally' }))
    await waitFor(() => {
      expect(openWithDefaultApp).toHaveBeenCalledWith('/tmp/shannon/report.pdf')
    })
  })

  it('caps the preview window at 200 pages and offers open-externally past the cap', async () => {
    const doc = makeFakeDoc(250)
    getDocument.mockReturnValue({ promise: Promise.resolve(doc), destroy: vi.fn(async () => {}) })

    render(<PdfPreview {...PROPS} />)

    // 250-page doc → indicator shows the capped window, not the real total.
    expect(await screen.findByText('Page 1 / 200')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Open externally' })).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Next page' }))
    expect(await screen.findByText('Page 2 / 200')).toBeInTheDocument()
  })

  it('closes from the dialog close button', async () => {
    const doc = makeFakeDoc(1)
    getDocument.mockReturnValue({ promise: Promise.resolve(doc), destroy: vi.fn(async () => {}) })
    const onClose = vi.fn()

    render(<PdfPreview {...PROPS} onClose={onClose} />)
    await screen.findByText('Page 1 / 1')

    fireEvent.click(screen.getByRole('button', { name: 'Close' }))
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('hands the worker URL to pdf.js GlobalWorkerOptions on module load', async () => {
    // The `?url` import resolves against the installed pdfjs-dist build; the
    // assignment must have run by the time the component module loaded.
    await import('../PdfPreview')
    expect(GlobalWorkerOptions.workerSrc).toContain('pdf.worker.min.mjs')
  })
})
