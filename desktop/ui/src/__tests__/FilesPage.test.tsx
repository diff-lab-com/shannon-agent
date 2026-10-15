// office Wave 2 B9' — FilesPage tests: mock-data rendering, favorite toggle
// hitting the wrapper, missing-file greying, favorites lens, empty state.
// R2 Wave C′: renders inside MemoryRouter (the page now owns a 来源会话
// jump via useNavigate) and covers the session-stamped jump button.
// R2 Wave B1: the 320px preview panel — wide-mode selection renders the
// card (fields, source jump, extraction badge, retracted actions), the
// honest absences (no session_id → no jump, no extracted_segments → no
// badge), the three ways to deselect (re-click, close, Esc), and the
// <1280px fallback that keeps the inline cell actions.
// R2 final sweep: the pagehead subtitle (design 11:138), the search box
// (11:141-144 — name/path filtering, pill-count linkage, lens stacking,
// Esc-to-clear) and the readable source-session title on the jump
// (session_title present → title; absent → generic label fallback).

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, waitFor, within } from '@testing-library/react'
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
    // R2 B2 contract: extraction report exists for this one → the preview
    // card may show the 已提取 N 段 badge. notes.md/gone.md carry no field
    // and must show none (honesty contract).
    extracted_segments: 12,
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

// R2 B1: the wide/narrow preview branch is JS-driven via matchMedia. The
// global setup stubs matchMedia so only prefers-reduced-motion can match
// (i.e. every test defaults to the narrow <1280px form). These helpers pin
// the breakpoint explicitly and restore the narrow default afterwards.
const installMatchMedia = (wide: boolean) => {
  Object.defineProperty(window, 'matchMedia', {
    writable: true,
    value: vi.fn().mockImplementation((query: string) => ({
      matches: wide ? /min-width:\s*1280px/.test(query) : /prefers-reduced-motion:\s*reduce/i.test(query),
      media: query,
      onchange: null,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })),
  })
}

beforeEach(() => {
  vi.mocked(api.listFileIndex).mockReset().mockResolvedValue(ENTRIES.map(e => ({ ...e })))
  vi.mocked(api.pathExists).mockReset().mockImplementation((p: string) =>
    Promise.resolve(p !== '/Users/demo/Documents/gone.md'))
  vi.mocked(api.setFileIndexFavorite).mockReset().mockResolvedValue(undefined)
})

afterEach(() => installMatchMedia(false))

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

// ---- R2 Wave B1: the 320px preview panel --------------------------------

describe('FilesPage — preview panel (R2 B1)', () => {
  it('≥1280px: placeholder before selection; selecting a card renders name, size, kind, badge, jump and actions', async () => {
    installMatchMedia(true)
    const switchSession = vi.fn().mockResolvedValue(undefined)
    renderFilesPage(switchSession)
    const rows = await screen.findAllByTestId('files-row')

    // Nothing selected → guidance placeholder, no card.
    expect(screen.getByTestId('files-preview-empty')).toBeInTheDocument()
    expect(screen.queryByTestId('files-preview-card')).not.toBeInTheDocument()

    // Select q3-review.pptx (first cell).
    fireEvent.click(within(rows[0]).getByTestId('files-row-select'))
    expect(rows[0].getAttribute('data-selected')).toBe('true')

    const card = screen.getByTestId('files-preview-card')
    expect(card).toHaveTextContent('q3-review.pptx')
    expect(card).toHaveTextContent('2.4 MB')
    // Extension-grouped kind label in the preview zone (.pptx → Slides).
    expect(card).toHaveTextContent('Slides')
    // extracted_segments present → the badge with its count (B2 contract).
    expect(screen.getByTestId('files-preview-extracted')).toHaveTextContent('Extracted 12 segments')
    // session_id present → the 来源会话 jump now lives in the panel…
    expect(screen.getByTestId('files-preview-source-jump')).toBeInTheDocument()
    // …and still reuses jumpToSource (switchSession → /chat).
    fireEvent.click(screen.getByTestId('files-preview-source-jump'))
    await waitFor(() =>
      expect(switchSession).toHaveBeenCalledWith('0a1960aa-9c1d-4a5e-b8f0-31f2d1c4091a'),
    )

    // The retracted actions hit the same OS wrappers.
    fireEvent.click(screen.getByTestId('files-preview-open'))
    await waitFor(() =>
      expect(api.openWithDefaultApp).toHaveBeenCalledWith('/Users/demo/Documents/q3-review.pptx'),
    )
    fireEvent.click(screen.getByTestId('files-preview-reveal'))
    await waitFor(() =>
      expect(api.revealInFolder).toHaveBeenCalledWith('/Users/demo/Documents/q3-review.pptx'),
    )
    // Favorite from the panel flips the same persisted flag (was true).
    fireEvent.click(screen.getByTestId('files-preview-favorite'))
    await waitFor(() =>
      expect(api.setFileIndexFavorite).toHaveBeenCalledWith('/Users/demo/Documents/q3-review.pptx', false),
    )
  })

  it('≥1280px: no session_id → no jump; no extracted_segments → no badge (honest absence)', async () => {
    installMatchMedia(true)
    renderFilesPage()
    const rows = await screen.findAllByTestId('files-row')

    fireEvent.click(within(rows[1]).getByTestId('files-row-select')) // notes.md
    const card = screen.getByTestId('files-preview-card')
    expect(card).toHaveTextContent('notes.md')
    expect(screen.queryByTestId('files-preview-source-jump')).not.toBeInTheDocument()
    expect(screen.queryByTestId('files-preview-extracted')).not.toBeInTheDocument()
  })

  it('≥1280px: selecting a missing file shows the missing note and disables open/reveal in the panel', async () => {
    installMatchMedia(true)
    renderFilesPage()
    const rows = await screen.findAllByTestId('files-row')

    fireEvent.click(within(rows[2]).getByTestId('files-row-select')) // gone.md
    expect(screen.getByTestId('files-preview-missing')).toBeInTheDocument()
    expect(screen.getByTestId('files-preview-open')).toBeDisabled()
    expect(screen.getByTestId('files-preview-reveal')).toBeDisabled()
  })

  it('selection toggles off via the same card, the close button, and Esc', async () => {
    installMatchMedia(true)
    renderFilesPage()
    const rows = await screen.findAllByTestId('files-row')
    const select = within(rows[0]).getByTestId('files-row-select')

    // Re-click the same cell → back to the placeholder.
    fireEvent.click(select)
    expect(screen.getByTestId('files-preview-card')).toBeInTheDocument()
    fireEvent.click(select)
    expect(screen.getByTestId('files-preview-empty')).toBeInTheDocument()

    // Panel close button.
    fireEvent.click(select)
    fireEvent.click(screen.getByTestId('files-preview-close'))
    expect(screen.getByTestId('files-preview-empty')).toBeInTheDocument()

    // Esc anywhere on the page.
    fireEvent.click(select)
    fireEvent.keyDown(window, { key: 'Escape' })
    expect(screen.getByTestId('files-preview-empty')).toBeInTheDocument()
    expect(screen.queryByTestId('files-preview-card')).not.toBeInTheDocument()
  })

  it('<1280px: the panel is not rendered and the cells keep their inline actions (fallback form)', async () => {
    installMatchMedia(false)
    renderFilesPage()
    const rows = await screen.findAllByTestId('files-row')

    expect(screen.queryByTestId('files-preview-panel')).not.toBeInTheDocument()
    // Inline open/reveal still on the cell — the pre-B1 form.
    expect(within(rows[0]).getByRole('button', { name: 'Open attachment' })).toBeInTheDocument()
    expect(within(rows[0]).getByRole('button', { name: 'Show in folder' })).toBeInTheDocument()
    expect(within(rows[0]).getByTestId('files-row-source-jump')).toBeInTheDocument()
  })

  it('≥1280px: the cell-level open/reveal/source actions are retracted into the panel', async () => {
    installMatchMedia(true)
    renderFilesPage()
    const rows = await screen.findAllByTestId('files-row')

    expect(within(rows[0]).queryByRole('button', { name: 'Open attachment' })).not.toBeInTheDocument()
    expect(within(rows[0]).queryByRole('button', { name: 'Show in folder' })).not.toBeInTheDocument()
    expect(within(rows[0]).queryByTestId('files-row-source-jump')).not.toBeInTheDocument()
    // The favorite star stays on the cell at both widths.
    expect(within(rows[0]).getByRole('button', { name: 'Remove from favorites' })).toBeInTheDocument()
  })
})

// ---- R2 final sweep: subtitle, search, readable source-session title ----

describe('FilesPage — R2 final sweep (subtitle, search, title join)', () => {
  const TITLED_ROWS: FileIndexEntry[] = [
    {
      path: '/Users/demo/Documents/q3-review.pptx',
      name: 'q3-review.pptx',
      size_bytes: 2_483_112,
      registered_at: new Date(Date.now() - 2 * 3600_000).toISOString(),
      favorite: false,
      source: 'generated',
      session_id: '0a1960aa-9c1d-4a5e-b8f0-31f2d1c4091a',
      // Backend joined a readable rail title → the jump reads it.
      session_title: 'Q3 roadmap brainstorm',
    },
    {
      path: '/Users/demo/Downloads/notes.md',
      name: 'notes.md',
      size_bytes: 8_210,
      registered_at: new Date(Date.now() - 26 * 3600_000).toISOString(),
      favorite: false,
      source: 'attachment',
      // Stamped but unresolvable/off-rail (or still placeholder-titled) →
      // no session_title → the generic label fallback.
      session_id: '11111111-2222-3333-4444-555555555555',
    },
  ]

  it('renders the design subtitle under the H1 (design 11:138)', async () => {
    renderFilesPage()
    expect(await screen.findByRole('heading', { name: 'Files' })).toBeInTheDocument()
    expect(
      screen.getByText('A unified index of session attachments and agent output — previewable and traceable.'),
    ).toBeInTheDocument()
  })

  it('search filters by name or path (case-insensitive) and the pill counts ride along', async () => {
    renderFilesPage()
    await screen.findAllByTestId('files-row')
    const input = screen.getByTestId('files-search') as HTMLInputElement

    // Name substring narrows to notes.md; every pill count answers too.
    fireEvent.change(input, { target: { value: 'notes' } })
    await waitFor(() => expect(screen.getAllByTestId('files-row')).toHaveLength(1))
    expect(screen.getAllByTestId('files-row')[0]).toHaveTextContent('notes.md')
    expect(screen.getByRole('button', { name: /All/ })).toHaveTextContent('1')
    expect(screen.getByRole('button', { name: /Attachments/ })).toHaveTextContent('1')
    expect(screen.getByRole('button', { name: /Agent outputs/ })).toHaveTextContent('0')

    // Path substring matches too — case-insensitively.
    fireEvent.change(input, { target: { value: 'DOCUMENTS/GONE' } })
    await waitFor(() => expect(screen.getAllByTestId('files-row')).toHaveLength(1))
    expect(screen.getAllByTestId('files-row')[0]).toHaveTextContent('gone.md')

    // No hit at all → the empty state, not a silent blank grid.
    fireEvent.change(input, { target: { value: 'no-such-file-xyz' } })
    await waitFor(() => expect(screen.getByText(/No files yet/)).toBeInTheDocument())

    // Esc clears the query; the full list returns and the input empties.
    fireEvent.keyDown(input, { key: 'Escape' })
    await waitFor(() => expect(screen.getAllByTestId('files-row')).toHaveLength(3))
    expect(input.value).toBe('')
  })

  it('search stacks with the lens pills (intersection, never replacement)', async () => {
    renderFilesPage()
    await screen.findAllByTestId('files-row')
    const input = screen.getByTestId('files-search')

    // 'review' matches only q3-review.pptx (a generated row).
    fireEvent.change(input, { target: { value: 'review' } })
    fireEvent.click(screen.getByRole('button', { name: /Agent outputs/ }))
    await waitFor(() => expect(screen.getAllByTestId('files-row')).toHaveLength(1))
    expect(screen.getAllByTestId('files-row')[0]).toHaveTextContent('q3-review.pptx')

    // The same query under the Attachments lens is empty: notes.md passes
    // the lens but not the query → the honest empty state.
    fireEvent.click(screen.getByRole('button', { name: /Attachments/ }))
    await waitFor(() => expect(screen.queryByTestId('files-row')).not.toBeInTheDocument())
    expect(screen.getByText(/No files yet/)).toBeInTheDocument()
  })

  it('jump shows the joined session title and falls back to the generic label when absent', async () => {
    vi.mocked(api.listFileIndex).mockResolvedValue(TITLED_ROWS.map(e => ({ ...e })))
    renderFilesPage()
    const rows = await screen.findAllByTestId('files-row')

    // Titled row: the readable session title is the button's label.
    const titledRow = rows.find(r => r.textContent?.includes('q3-review.pptx'))!
    expect(within(titledRow).getByTestId('files-row-source-jump')).toHaveTextContent('Q3 roadmap brainstorm')
    // Stamped-but-untitled row: the generic 「来源会话」 label fallback.
    expect(within(rows.find(r => r.textContent?.includes('notes.md'))!).getByTestId('files-row-source-jump'))
      .toHaveTextContent('Source session')
  })

  it('≥1280px: the preview card jump prefers the readable session title and still navigates', async () => {
    installMatchMedia(true)
    const switchSession = vi.fn().mockResolvedValue(undefined)
    vi.mocked(api.listFileIndex).mockResolvedValue(TITLED_ROWS.map(e => ({ ...e })))
    renderFilesPage(switchSession)
    const rows = await screen.findAllByTestId('files-row')

    // Select the titled deck → the panel's jump reads the session title.
    fireEvent.click(within(rows[0]).getByTestId('files-row-select'))
    const jump = screen.getByTestId('files-preview-source-jump')
    expect(jump).toHaveTextContent('Q3 roadmap brainstorm')
    expect(jump).toHaveAttribute('aria-label', 'Q3 roadmap brainstorm')

    fireEvent.click(jump)
    await waitFor(() =>
      expect(switchSession).toHaveBeenCalledWith('0a1960aa-9c1d-4a5e-b8f0-31f2d1c4091a'),
    )
  })
})
