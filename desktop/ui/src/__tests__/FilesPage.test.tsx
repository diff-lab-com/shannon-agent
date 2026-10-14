// office Wave 2 B9' — FilesPage tests: mock-data rendering, favorite toggle
// hitting the wrapper, missing-file greying, favorites lens, empty state.
// R2 Wave C′: renders inside MemoryRouter (the page now owns a 来源会话
// jump via useNavigate) and covers the session-stamped jump button.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import FilesPage from '@/pages/FilesPage'
import { SessionContext } from '@/context/SessionContext'
import * as api from '@/lib/tauri-api'
import type * as TauriApiModule from '@/lib/tauri-api'
import type { FileIndexEntry } from '@/types'

vi.mock('@/lib/tauri-api', async (importOriginal) => ({
  ...(await importOriginal<typeof TauriApiModule>()),
  listFileIndex: vi.fn(),
  pathExists: vi.fn(),
  setFileIndexFavorite: vi.fn().mockResolvedValue(undefined),
  openWithDefaultApp: vi.fn().mockResolvedValue(undefined),
  revealInFolder: vi.fn().mockResolvedValue(undefined),
}))

const ENTRIES: FileIndexEntry[] = [
  {
    path: '/Users/demo/Documents/q3-review.pptx',
    name: 'q3-review.pptx',
    size_bytes: 2_483_112,
    registered_at: new Date(Date.now() - 2 * 3600_000).toISOString(),
    favorite: true,
    source: 'generated',
    // R2 Wave C′: stamped at registration → the row grows a 来源会话 jump.
    session_id: '0a1960aa-9c1d-4a5e-b8f0-31f2d1c4091a',
  },
  {
    path: '/Users/demo/Downloads/notes.md',
    name: 'notes.md',
    size_bytes: 8_210,
    registered_at: new Date(Date.now() - 26 * 3600_000).toISOString(),
    favorite: false,
    source: 'attachment',
  },
  {
    path: '/Users/demo/Documents/gone.md',
    name: 'gone.md',
    size_bytes: null,
    registered_at: new Date(Date.now() - 72 * 3600_000).toISOString(),
    favorite: false,
    source: 'generated',
  },
]

const renderFilesPage = (switchSession?: (id: string) => Promise<void>) =>
  render(
    switchSession ? (
      <MemoryRouter>
        <SessionContext.Provider value={{ switchSession } as never}>
          <FilesPage />
        </SessionContext.Provider>
      </MemoryRouter>
    ) : (
      <MemoryRouter>
        <FilesPage />
      </MemoryRouter>
    ),
  )

beforeEach(() => {
  vi.mocked(api.listFileIndex).mockReset().mockResolvedValue(ENTRIES.map(e => ({ ...e })))
  vi.mocked(api.pathExists).mockReset().mockImplementation((p: string) =>
    Promise.resolve(p !== '/Users/demo/Documents/gone.md'))
  vi.mocked(api.setFileIndexFavorite).mockReset().mockResolvedValue(undefined)
})

describe('FilesPage', () => {
  it('renders the mock index newest-first with name, size and relative time', async () => {
    renderFilesPage()
    const rows = await screen.findAllByTestId('files-row')
    expect(rows).toHaveLength(3)
    expect(rows[0]).toHaveTextContent('q3-review.pptx')
    expect(rows[0]).toHaveTextContent('2.4 MB')
    expect(rows[1]).toHaveTextContent('notes.md')
    // Missing badge only on the row whose path probe returned false.
    expect(rows[2]).toHaveTextContent('Original file moved or deleted')
    expect(rows[0]).not.toHaveTextContent('Original file moved or deleted')
  })

  it('greys out a missing row and disables its open/reveal actions', async () => {
    renderFilesPage()
    const rows = await screen.findAllByTestId('files-row')
    const missingRow = rows.find(r => r.textContent?.includes('gone.md'))!
    expect(missingRow.getAttribute('data-missing')).toBe('true')
    expect(missingRow.className).toMatch(/opacity-50/)
    // Open/reveal are inert for a file that is not there.
    const openBtn = missingRow.querySelector('button[aria-label="Open attachment"]') as HTMLButtonElement
    const revealBtn = missingRow.querySelector('button[aria-label="Show in folder"]') as HTMLButtonElement
    expect(openBtn).toBeDisabled()
    expect(revealBtn).toBeDisabled()
  })

  it('favorite toggle calls setFileIndexFavorite with the flipped value', async () => {
    renderFilesPage()
    await screen.findAllByTestId('files-row')
    // notes.md (not favorite) — its star's accessible name is "Add to favorites".
    const star = screen.getAllByRole('button', { name: 'Add to favorites' })
      .find(btn => btn.closest('[data-testid="files-row"]')?.textContent?.includes('notes.md'))!
    fireEvent.click(star)
    await waitFor(() =>
      expect(api.setFileIndexFavorite).toHaveBeenCalledWith('/Users/demo/Downloads/notes.md', true),
    )
    // Optimistic UI: the star flips immediately.
    await waitFor(() => expect(star.getAttribute('aria-pressed')).toBe('true'))
  })

  it('reverts the star and keeps the page alive when the wrapper rejects', async () => {
    vi.mocked(api.setFileIndexFavorite).mockRejectedValueOnce(new Error('disk unavailable'))
    renderFilesPage()
    await screen.findAllByTestId('files-row')
    const star = screen.getAllByRole('button', { name: 'Add to favorites' })
      .find(btn => btn.closest('[data-testid="files-row"]')?.textContent?.includes('notes.md'))!
    fireEvent.click(star)
    await waitFor(() => expect(api.setFileIndexFavorite).toHaveBeenCalled())
    await waitFor(() => expect(star.getAttribute('aria-pressed')).toBe('false'))
  })

  it('the Favorites lens shows only favorite entries; All restores the full list', async () => {
    renderFilesPage()
    await screen.findAllByTestId('files-row')

    // The lens pills carry live counts ("Favorites 1") — match by prefix.
    fireEvent.click(screen.getByRole('button', { name: /Favorites/ }))
    await waitFor(() => expect(screen.getAllByTestId('files-row')).toHaveLength(1))
    expect(screen.getAllByTestId('files-row')[0]).toHaveTextContent('q3-review.pptx')

    fireEvent.click(screen.getByRole('button', { name: /All/ }))
    await waitFor(() => expect(screen.getAllByTestId('files-row')).toHaveLength(3))
  })

  it('the source and missing lenses filter by tag and probe result', async () => {
    renderFilesPage()
    await screen.findAllByTestId('files-row')

    // 附件: one attachment row (notes.md).
    fireEvent.click(screen.getByRole('button', { name: /Attachments/ }))
    await waitFor(() => expect(screen.getAllByTestId('files-row')).toHaveLength(1))
    expect(screen.getAllByTestId('files-row')[0]).toHaveTextContent('notes.md')

    // Agent 产出: the two generated rows.
    fireEvent.click(screen.getByRole('button', { name: /Agent outputs/ }))
    await waitFor(() => expect(screen.getAllByTestId('files-row')).toHaveLength(2))

    // 已丢失: only the row whose path probe confirmed the file is gone.
    fireEvent.click(screen.getByRole('button', { name: /Missing/ }))
    await waitFor(() => expect(screen.getAllByTestId('files-row')).toHaveLength(1))
    expect(screen.getAllByTestId('files-row')[0]).toHaveTextContent('gone.md')
    expect(screen.getByTestId('files-row-missing')).toBeInTheDocument()
  })

  it('shows the empty state when the index is empty', async () => {
    vi.mocked(api.listFileIndex).mockResolvedValue([])
    renderFilesPage()
    await waitFor(() => expect(screen.getByText(/No files yet/)).toBeInTheDocument())
    expect(screen.queryByTestId('files-list')).not.toBeInTheDocument()
  })

  // ---- R2 Wave C′: 来源会话 provenance jump -----------------------------

  it('shows the source jump only on session-stamped rows and switches to it', async () => {
    const switchSession = vi.fn().mockResolvedValue(undefined)
    renderFilesPage(switchSession)
    const rows = await screen.findAllByTestId('files-row')

    // q3-review.pptx carries a session_id → exactly one jump button.
    const jumps = screen.getAllByTestId('files-row-source-jump')
    expect(jumps).toHaveLength(1)
    expect(jumps[0].closest('[data-testid="files-row"]')).toHaveTextContent('q3-review.pptx')

    fireEvent.click(jumps[0])
    await waitFor(() =>
      expect(switchSession).toHaveBeenCalledWith('0a1960aa-9c1d-4a5e-b8f0-31f2d1c4091a'),
    )

    // Historical / unstamped rows render no jump — honest absence.
    const notesRow = rows.find(r => r.textContent?.includes('notes.md'))!
    expect(notesRow.querySelector('[data-testid="files-row-source-jump"]')).toBeNull()
  })

  it('renders without a SessionContext provider (bare test mount) — jump click is a no-op', async () => {
    renderFilesPage()
    const jumps = await screen.findAllByTestId('files-row-source-jump')
    expect(jumps).toHaveLength(1)
    // sessionCtx is null → the click resolves without throwing.
    fireEvent.click(jumps[0])
  })
})
