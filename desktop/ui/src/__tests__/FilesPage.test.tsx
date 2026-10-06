// office Wave 2 B9' — FilesPage tests: mock-data rendering, favorite toggle
// hitting the wrapper, missing-file greying, favorites lens, empty state.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import FilesPage from '@/pages/FilesPage'
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

beforeEach(() => {
  vi.mocked(api.listFileIndex).mockReset().mockResolvedValue(ENTRIES.map(e => ({ ...e })))
  vi.mocked(api.pathExists).mockReset().mockImplementation((p: string) =>
    Promise.resolve(p !== '/Users/demo/Documents/gone.md'))
  vi.mocked(api.setFileIndexFavorite).mockReset().mockResolvedValue(undefined)
})

describe('FilesPage', () => {
  it('renders the mock index newest-first with name, size and relative time', async () => {
    render(<FilesPage />)
    const rows = await screen.findAllByTestId('files-row')
    expect(rows).toHaveLength(3)
    expect(rows[0]).toHaveTextContent('q3-review.pptx')
    expect(rows[0]).toHaveTextContent('2.4 MB')
    expect(rows[1]).toHaveTextContent('notes.md')
    // Missing badge only on the row whose path probe returned false.
    expect(rows[2]).toHaveTextContent('File moved or deleted')
    expect(rows[0]).not.toHaveTextContent('File moved or deleted')
  })

  it('greys out a missing row and disables its open/reveal actions', async () => {
    render(<FilesPage />)
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
    render(<FilesPage />)
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
    render(<FilesPage />)
    await screen.findAllByTestId('files-row')
    const star = screen.getAllByRole('button', { name: 'Add to favorites' })
      .find(btn => btn.closest('[data-testid="files-row"]')?.textContent?.includes('notes.md'))!
    fireEvent.click(star)
    await waitFor(() => expect(api.setFileIndexFavorite).toHaveBeenCalled())
    await waitFor(() => expect(star.getAttribute('aria-pressed')).toBe('false'))
  })

  it('the Favorites lens shows only favorite entries; All restores the full list', async () => {
    render(<FilesPage />)
    await screen.findAllByTestId('files-row')

    // The lens's Favorites leg is the star OUTSIDE any row (the rows' stars
    // share the same accessible name).
    const lensStar = screen.getAllByRole('button', { name: 'Add to favorites' })
      .find(btn => btn.closest('[data-testid="files-row"]') == null)!
    fireEvent.click(lensStar)
    await waitFor(() => expect(screen.getAllByTestId('files-row')).toHaveLength(1))
    expect(screen.getAllByTestId('files-row')[0]).toHaveTextContent('q3-review.pptx')

    fireEvent.click(screen.getByRole('button', { name: 'All' }))
    await waitFor(() => expect(screen.getAllByTestId('files-row')).toHaveLength(3))
  })

  it('shows the empty state when the index is empty', async () => {
    vi.mocked(api.listFileIndex).mockResolvedValue([])
    render(<FilesPage />)
    await waitFor(() => expect(screen.getByText(/No files yet/)).toBeInTheDocument())
    expect(screen.queryByTestId('files-list')).not.toBeInTheDocument()
  })
})
