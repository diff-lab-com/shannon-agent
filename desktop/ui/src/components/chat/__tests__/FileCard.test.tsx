import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { save } from '@tauri-apps/plugin-dialog'
import { toast } from 'sonner'

import { FileCard } from '../FileCard'

// Per-file tauri-api mock (same pattern as artifact/__tests__/HtmlRenderer):
// the factory replaces the global setup mock for this file, so the three
// wrappers FileCard touches are directly assertable. `save` keeps the global
// setup mock (@tauri-apps/plugin-dialog) — default resolution is null, i.e.
// the user cancels the dialog.
const { openWithDefaultApp, revealInFolder, copyFile, registerFileIndexEntry } = vi.hoisted(() => ({
  openWithDefaultApp: vi.fn(),
  revealInFolder: vi.fn(),
  copyFile: vi.fn(),
  // office Wave 2 B9': the card indexes itself on mount (fire-and-forget).
  registerFileIndexEntry: vi.fn(),
}))

vi.mock('@/lib/tauri-api', () => ({
  openWithDefaultApp,
  revealInFolder,
  copyFile,
  registerFileIndexEntry,
}))

// B8b: FileCard lazy-loads PdfPreview — stub the chunk so the click test
// asserts the wiring (mount with the card's path) without pulling pdf.js in.
vi.mock('../PdfPreview', () => ({
  PdfPreview: (props: { path: string; name: string; onClose: () => void }) => (
    <div data-testid="pdf-preview-stub" data-path={props.path} data-name={props.name} />
  ),
}))

vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn(), message: vi.fn() },
}))

const mockedSave = vi.mocked(save)

const PROPS = { name: 'report.docx', path: '/tmp/shannon/report.docx', sizeBytes: 2048 }

beforeEach(() => {
  vi.clearAllMocks()
  mockedSave.mockResolvedValue(null)
  openWithDefaultApp.mockResolvedValue(undefined)
  revealInFolder.mockResolvedValue(undefined)
  copyFile.mockResolvedValue(undefined)
  registerFileIndexEntry.mockResolvedValue(undefined)
})

afterEach(() => {
  cleanup()
})

describe('FileCard', () => {
  it('renders the file name, a human-readable size and the three actions', () => {
    render(<FileCard {...PROPS} />)
    expect(screen.getByText('report.docx')).toBeInTheDocument()
    expect(screen.getByText('2.0 KB')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Open attachment' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Show in folder' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Save as…' })).toBeInTheDocument()
  })

  it('omits the size label when sizeBytes is not provided', () => {
    render(<FileCard name="notes.txt" path="/tmp/notes.txt" />)
    expect(screen.getByText('notes.txt')).toBeInTheDocument()
    expect(screen.queryByText(/KB/)).not.toBeInTheDocument()
  })

  it('open hands the path to openWithDefaultApp', () => {
    render(<FileCard {...PROPS} />)
    fireEvent.click(screen.getByRole('button', { name: 'Open attachment' }))
    expect(openWithDefaultApp).toHaveBeenCalledWith('/tmp/shannon/report.docx')
  })

  it('reveal hands the path to revealInFolder', () => {
    render(<FileCard {...PROPS} />)
    fireEvent.click(screen.getByRole('button', { name: 'Show in folder' }))
    expect(revealInFolder).toHaveBeenCalledWith('/tmp/shannon/report.docx')
  })

  it('save as copies to the chosen destination and toasts success', async () => {
    mockedSave.mockResolvedValueOnce('/Users/ed/Downloads/report-final.docx')
    render(<FileCard {...PROPS} />)
    fireEvent.click(screen.getByRole('button', { name: 'Save as…' }))
    await waitFor(() => {
      expect(copyFile).toHaveBeenCalledWith('/tmp/shannon/report.docx', '/Users/ed/Downloads/report-final.docx')
    })
    await waitFor(() => {
      expect(toast.success).toHaveBeenCalledWith('Saved to /Users/ed/Downloads/report-final.docx')
    })
  })

  it('a cancelled save dialog never copies and stays silent', async () => {
    render(<FileCard {...PROPS} />)
    fireEvent.click(screen.getByRole('button', { name: 'Save as…' }))
    await waitFor(() => expect(mockedSave).toHaveBeenCalledTimes(1))
    expect(copyFile).not.toHaveBeenCalled()
    expect(toast.success).not.toHaveBeenCalled()
    expect(toast.error).not.toHaveBeenCalled()
  })

  it('a failed copy toasts the failure message with the error cause', async () => {
    copyFile.mockRejectedValueOnce(new Error('disk full'))
    mockedSave.mockResolvedValueOnce('/tmp/dest/report.docx')
    render(<FileCard {...PROPS} />)
    fireEvent.click(screen.getByRole('button', { name: 'Save as…' }))
    await waitFor(() => {
      expect(toast.error).toHaveBeenCalledWith('Save failed', { description: 'disk full' })
    })
    expect(toast.success).not.toHaveBeenCalled()
  })

  // ── Wave 1.5 B8b: PDF-only inline preview ──

  it('a pdf card shows a Preview button that mounts PdfPreview with the card path', async () => {
    render(<FileCard name="report.pdf" path="/tmp/shannon/report.pdf" />)
    expect(screen.getByRole('button', { name: 'Preview' })).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Preview' }))
    // lazy chunk resolves on the next microtask → findBy*
    const stub = await screen.findByTestId('pdf-preview-stub')
    expect(stub).toHaveAttribute('data-path', '/tmp/shannon/report.pdf')
    expect(stub).toHaveAttribute('data-name', 'report.pdf')
  })

  it('non-pdf cards keep the three-action layout without a preview button', () => {
    render(<FileCard {...PROPS} />)
    expect(screen.queryByRole('button', { name: 'Preview' })).not.toBeInTheDocument()
  })

  // ── office Wave 3 C2: csv-only "Batch run" affordance ──

  it('a csv card shows the Batch run button', () => {
    render(<FileCard name="inventory.csv" path="/tmp/shannon/inventory.csv" />)
    expect(screen.getByTestId('file-card-batch-run')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Batch run over table rows' })).toBeInTheDocument()
  })

  it('a docx card has no Batch run button', () => {
    render(<FileCard {...PROPS} />)
    expect(screen.queryByTestId('file-card-batch-run')).not.toBeInTheDocument()
  })

  // ── G3b P1-4: extraction detail area ──

  const OFFICE_EXTRACTION = {
    path: '/tmp/shannon/report.docx',
    kind: 'docx',
    extracted: true,
    sections_total: 23,
    sections_inlined: 8,
    truncated: true,
    cache_path: '/home/u/.shannon/cache/extracted/abc123.txt',
  }

  it('a card with an extraction report shows the summary, cache path and view action', () => {
    render(<FileCard {...PROPS} extraction={OFFICE_EXTRACTION} />)
    const detail = screen.getByTestId('file-card-extraction')
    expect(detail).toHaveTextContent('Extracted 23 sections, first 8 inlined')
    expect(detail).toHaveTextContent('/home/u/.shannon/cache/extracted/abc123.txt')
    expect(screen.getByTestId('file-card-view-extracted')).toBeInTheDocument()
  })

  it('"View extracted text" opens the cached file, not the source', () => {
    render(<FileCard {...PROPS} extraction={OFFICE_EXTRACTION} />)
    fireEvent.click(screen.getByTestId('file-card-view-extracted'))
    expect(openWithDefaultApp).toHaveBeenCalledWith('/home/u/.shannon/cache/extracted/abc123.txt')
  })

  it('a truncated PDF report carries the 50 KiB wording; failure reports say the model got nothing', () => {
    const { rerender } = render(
      <FileCard
        {...PROPS}
        name="manual.pdf"
        path="/tmp/shannon/manual.pdf"
        extraction={{ ...OFFICE_EXTRACTION, path: '/tmp/shannon/manual.pdf', kind: 'pdf', sections_total: 0, sections_inlined: 0 }}
      />,
    )
    expect(screen.getByTestId('file-card-extraction')).toHaveTextContent('PDF inlines the first 50 KiB')

    rerender(
      <FileCard
        {...PROPS}
        extraction={{ ...OFFICE_EXTRACTION, extracted: false, sections_total: 0, sections_inlined: 0, cache_path: undefined }}
      />,
    )
    expect(screen.getByTestId('file-card-extraction')).toHaveTextContent(
      'Text extraction failed — no content reached the model',
    )
    // No cache → no view action.
    expect(screen.queryByTestId('file-card-view-extracted')).not.toBeInTheDocument()
  })

  it('cards without an extraction report keep the plain layout', () => {
    render(<FileCard {...PROPS} />)
    expect(screen.queryByTestId('file-card-extraction')).not.toBeInTheDocument()
    expect(screen.queryByTestId('file-card-view-extracted')).not.toBeInTheDocument()
  })
})
