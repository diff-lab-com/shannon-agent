import { describe, it, expect, vi, beforeEach } from 'vitest'
import { useState } from 'react'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import DataSourcesQuery from '@/components/extensions/DataSourcesQuery'
import * as api from '@/lib/tauri-api'
import {
  pushComposerDraft,
  resetPendingComposerDraftsForTests,
  useComposerDraftListener,
} from '@/lib/composerBridge'
import type * as composerBridgeModule from '@/lib/composerBridge'
import { toast } from 'sonner'

// Mock the tauri-api module
vi.mock('@/lib/tauri-api')

// Office Wave 2 B3 — the composer bridge. G5 P0-6: the REAL bridge runs here
// (spy-wrapped so tests still observe pushes) because the pending-draft
// queue is exactly what the cross-route test below exercises.
vi.mock('@/lib/composerBridge', async importOriginal => {
  const actual = await importOriginal<typeof composerBridgeModule>()
  return {
    ...actual,
    pushComposerDraft: vi.fn((text: string) => actual.pushComposerDraft(text)),
  }
})

vi.mock('sonner', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}))

/**
 * Minimal stand-in for the /chat page: mounts a composer-draft subscription
 * and renders every draft it receives. G5 P0-6 — after the cross-route push
 * + navigate, the queued draft must appear HERE.
 */
function DraftProbe() {
  const [drafts, setDrafts] = useState<string[]>([])
  useComposerDraftListener(text => setDrafts(prev => [...prev, text]))
  return (
    <div data-testid="chat-page">
      {drafts.map((d, i) => (
        <p key={i} data-testid="composer-draft">{d}</p>
      ))}
    </div>
  )
}

/** Render the query page at its real route with a /chat target behind it. */
function renderQueryPage() {
  return render(
    <MemoryRouter initialEntries={['/extensions/datasources']}>
      <Routes>
        <Route path="/extensions/datasources" element={<DataSourcesQuery />} />
        <Route path="/chat" element={<DraftProbe />} />
      </Routes>
    </MemoryRouter>,
  )
}

describe('DataSourcesQuery', () => {
  const mockListInstalledDataSources = vi.mocked(api.listInstalledDataSources)
  const mockQueryDataSource = vi.mocked(api.queryDataSource)
  const mockPushComposerDraft = vi.mocked(pushComposerDraft)
  const mockToastSuccess = vi.mocked(toast.success)

  beforeEach(() => {
    vi.clearAllMocks()
    resetPendingComposerDraftsForTests()
  })

  describe('Loading state', () => {
    it('shows loading spinner while fetching installed data sources', async () => {
      mockListInstalledDataSources.mockImplementation(
        () => new Promise(() => {}) // Never resolves
      )

      renderQueryPage()

      expect(screen.getByText(/loading installed data sources/i)).toBeInTheDocument()
    })
  })

  describe('Empty state', () => {
    it('shows empty state when no data sources are installed', async () => {
      mockListInstalledDataSources.mockResolvedValue([])

      renderQueryPage()

      await waitFor(() => {
        expect(screen.getByText(/no data sources installed/i)).toBeInTheDocument()
      })
    })
  })

  describe('Search form', () => {
    beforeEach(() => {
      mockListInstalledDataSources.mockResolvedValue([
        {
          slug: 'obsidian-vault',
          kind: 'obsidian',
          name: 'My Notes',
          path: '/path/to/vault',
          installed_at: '2024-01-01',
        },
      ])
    })

    it('renders search form when data sources are installed', async () => {
      renderQueryPage()

      await waitFor(() => {
        expect(screen.getByLabelText(/select data source/i)).toBeInTheDocument()
        expect(screen.getByPlaceholderText(/enter your search query/i)).toBeInTheDocument()
        expect(screen.getByRole('button', { name: /search/i })).toBeInTheDocument()
      })
    })

    it('populates data source dropdown', async () => {
      renderQueryPage()

      await waitFor(() => {
        const select = screen.getByLabelText(/select data source/i)
        expect(select).toBeInTheDocument()
      })

      const select = screen.getByLabelText(/select data source/i) as HTMLSelectElement
      const options = Array.from(select.options)
      expect(options).toHaveLength(2) // placeholder + 1 source
      expect(options[1].textContent).toContain('My Notes')
      expect(options[1].textContent).toContain('obsidian-vault')
    })

    it('disables search button when form is incomplete', async () => {
      renderQueryPage()

      await waitFor(() => {
        expect(screen.getByRole('button', { name: /search/i })).toBeInTheDocument()
      })

      const searchButton = screen.getByRole('button', { name: /search/i })
      expect(searchButton).toBeDisabled()
    })

    it('enables search button when form is complete', async () => {
      const user = userEvent.setup()
      renderQueryPage()

      await waitFor(() => {
        expect(screen.getByLabelText(/select data source/i)).toBeInTheDocument()
      })

      const select = screen.getByLabelText(/select data source/i)
      const input = screen.getByPlaceholderText(/enter your search query/i)
      const searchButton = screen.getByRole('button', { name: /search/i })

      await user.selectOptions(select, 'obsidian-vault')
      await user.type(input, 'test query')

      expect(searchButton).toBeEnabled()
    })

    it('calls queryDataSource with correct arguments on search', async () => {
      const user = userEvent.setup()
      mockQueryDataSource.mockResolvedValue({
        items: [],
        total: 0,
      })

      renderQueryPage()

      await waitFor(() => {
        expect(screen.getByLabelText(/select data source/i)).toBeInTheDocument()
      })

      const select = screen.getByLabelText(/select data source/i)
      const input = screen.getByPlaceholderText(/enter your search query/i)
      const searchButton = screen.getByRole('button', { name: /search/i })

      await user.selectOptions(select, 'obsidian-vault')
      await user.type(input, 'test query')
      await user.click(searchButton)

      expect(mockQueryDataSource).toHaveBeenCalledWith('obsidian-vault', 'test query')
      expect(mockQueryDataSource).toHaveBeenCalledTimes(1)
    })
  })

  describe('Loading state during search', () => {
    it('shows loading state while query is in progress', async () => {
      const user = userEvent.setup()
      mockListInstalledDataSources.mockResolvedValue([
        {
          slug: 'obsidian-vault',
          kind: 'obsidian',
          name: 'My Notes',
          path: '/path/to/vault',
          installed_at: '2024-01-01',
        },
      ])
      mockQueryDataSource.mockImplementation(
        () => new Promise(() => {}) // Never resolves
      )

      renderQueryPage()

      await waitFor(() => {
        expect(screen.getByLabelText(/select data source/i)).toBeInTheDocument()
      })

      const select = screen.getByLabelText(/select data source/i)
      const input = screen.getByPlaceholderText(/enter your search query/i)
      const searchButton = screen.getByRole('button', { name: /search/i })

      await user.selectOptions(select, 'obsidian-vault')
      await user.type(input, 'test query')
      await user.click(searchButton)

      expect(screen.getByRole('button', { name: /searching/i })).toBeInTheDocument()
    })
  })

  describe('Results display', () => {
    it('renders results after successful query', async () => {
      const user = userEvent.setup()
      mockListInstalledDataSources.mockResolvedValue([
        {
          slug: 'obsidian-vault',
          kind: 'obsidian',
          name: 'My Notes',
          path: '/path/to/vault',
          installed_at: '2024-01-01',
        },
      ])
      mockQueryDataSource.mockResolvedValue({
        items: [
          {
            title: 'Test Note',
            body: 'This is a test note content',
            url: 'https://example.com/note',
            kind: 'markdown',
            updated_at: '2024-01-01T00:00:00Z',
          },
        ],
        total: 1,
      })

      renderQueryPage()

      await waitFor(() => {
        expect(screen.getByLabelText(/select data source/i)).toBeInTheDocument()
      })

      const select = screen.getByLabelText(/select data source/i)
      const input = screen.getByPlaceholderText(/enter your search query/i)
      const searchButton = screen.getByRole('button', { name: /search/i })

      await user.selectOptions(select, 'obsidian-vault')
      await user.type(input, 'test')
      await user.click(searchButton)

      await waitFor(() => {
        expect(screen.getByText(/1 result/i)).toBeInTheDocument()
        expect(screen.getByText('Test Note')).toBeInTheDocument()
        expect(screen.getByText('This is a test note content')).toBeInTheDocument()
      })
    })

    it('shows empty results when query returns no items', async () => {
      const user = userEvent.setup()
      mockListInstalledDataSources.mockResolvedValue([
        {
          slug: 'obsidian-vault',
          kind: 'obsidian',
          name: 'My Notes',
          path: '/path/to/vault',
          installed_at: '2024-01-01',
        },
      ])
      mockQueryDataSource.mockResolvedValue({
        items: [],
        total: 0,
      })

      renderQueryPage()

      await waitFor(() => {
        expect(screen.getByLabelText(/select data source/i)).toBeInTheDocument()
      })

      const select = screen.getByLabelText(/select data source/i)
      const input = screen.getByPlaceholderText(/enter your search query/i)
      const searchButton = screen.getByRole('button', { name: /search/i })

      await user.selectOptions(select, 'obsidian-vault')
      await user.type(input, 'test')
      await user.click(searchButton)

      // Office Wave 2 B3 — empty results use the shared office.sources.noResults copy.
      await waitFor(() => {
        expect(screen.getByText(/no results for this query/i)).toBeInTheDocument()
      })
    })
  })

  describe('Error handling', () => {
    it('shows error message when query fails', async () => {
      const user = userEvent.setup()
      mockListInstalledDataSources.mockResolvedValue([
        {
          slug: 'obsidian-vault',
          kind: 'obsidian',
          name: 'My Notes',
          path: '/path/to/vault',
          installed_at: '2024-01-01',
        },
      ])
      mockQueryDataSource.mockRejectedValue(new Error('Connection failed'))

      renderQueryPage()

      await waitFor(() => {
        expect(screen.getByLabelText(/select data source/i)).toBeInTheDocument()
      })

      const select = screen.getByLabelText(/select data source/i)
      const input = screen.getByPlaceholderText(/enter your search query/i)
      const searchButton = screen.getByRole('button', { name: /search/i })

      await user.selectOptions(select, 'obsidian-vault')
      await user.type(input, 'test')
      await user.click(searchButton)

      await waitFor(() => {
        expect(screen.getByText(/query failed/i)).toBeInTheDocument()
        expect(screen.getByText(/connection failed/i)).toBeInTheDocument()
      })
    })
  })

  describe('Result card rendering', () => {
    it('renders item with all fields', async () => {
      const user = userEvent.setup()
      mockListInstalledDataSources.mockResolvedValue([
        {
          slug: 'obsidian-vault',
          kind: 'obsidian',
          name: 'My Notes',
          path: '/path/to/vault',
          installed_at: '2024-01-01',
        },
      ])
      mockQueryDataSource.mockResolvedValue({
        items: [
          {
            title: 'React Tutorial',
            body: 'Learn React hooks and state management',
            url: 'https://example.com/react',
            kind: 'markdown',
            updated_at: '2024-01-01T00:00:00Z',
          },
        ],
        total: 1,
        source_slug: 'obsidian-vault',
        source_name: 'My Notes',
      })

      renderQueryPage()

      await waitFor(() => {
        expect(screen.getByLabelText(/select data source/i)).toBeInTheDocument()
      })

      const select = screen.getByLabelText(/select data source/i)
      const input = screen.getByPlaceholderText(/enter your search query/i)
      const searchButton = screen.getByRole('button', { name: /search/i })

      await user.selectOptions(select, 'obsidian-vault')
      await user.type(input, 'react')
      await user.click(searchButton)

      await waitFor(() => {
        expect(screen.getByText('React Tutorial')).toBeInTheDocument()
        expect(screen.getByText('Learn React hooks and state management')).toBeInTheDocument()
        expect(screen.getByText('markdown')).toBeInTheDocument()
        expect(screen.getByRole('link', { name: /open/i })).toHaveAttribute('href', 'https://example.com/react')
      })
    })
  })

  // Office Wave 2 B3 — result cards hand their content to the chat composer
  // as source-attributed context blocks:
  //   [Source: <title|name>] (<url|path>)
  //   <body excerpt capped at 2000 chars>
  describe('Add to chat (Office Wave 2 B3)', () => {
    beforeEach(() => {
      mockListInstalledDataSources.mockResolvedValue([
        {
          slug: 'obsidian-vault',
          kind: 'obsidian',
          name: 'My Notes',
          path: '/path/to/vault',
          installed_at: '2024-01-01',
        },
      ])
    })

    async function queryWithItem(item: {
      id: string
      title: string
      body?: string | null
      url?: string | null
      kind: string
      updated_at?: string | null
    }) {
      const user = userEvent.setup()
      mockQueryDataSource.mockResolvedValue({ items: [item], total: 1, has_more: false })
      renderQueryPage()

      await waitFor(() => {
        expect(screen.getByLabelText(/select data source/i)).toBeInTheDocument()
      })

      await user.selectOptions(screen.getByLabelText(/select data source/i), 'obsidian-vault')
      await user.type(screen.getByPlaceholderText(/enter your search query/i), 'test')
      await user.click(screen.getByRole('button', { name: /search/i }))

      await waitFor(() => {
        expect(screen.getByRole('button', { name: /add to chat/i })).toBeInTheDocument()
      })
    }

    it('pushes a source-attributed context block into the composer draft', async () => {
      await queryWithItem({
        id: 'n1',
        title: 'Test Note',
        body: 'This is a test note content',
        url: 'https://example.com/note',
        kind: 'markdown',
        updated_at: null,
      })

      await userEvent.setup().click(screen.getByRole('button', { name: /add to chat/i }))

      expect(mockPushComposerDraft).toHaveBeenCalledTimes(1)
      const pushed = mockPushComposerDraft.mock.calls[0][0]
      expect(pushed).toContain('[Source: Test Note]')
      expect(pushed).toContain('(https://example.com/note)')
      expect(pushed).toContain('This is a test note content')
      expect(mockToastSuccess).toHaveBeenCalledWith('Added to the chat draft')
    })

    it('falls back to the installed source path when the item has no url', async () => {
      await queryWithItem({
        id: 'n2',
        title: 'Local note',
        body: 'hello world',
        url: null,
        kind: 'markdown',
        updated_at: null,
      })

      await userEvent.setup().click(screen.getByRole('button', { name: /add to chat/i }))

      const pushed = mockPushComposerDraft.mock.calls[0][0]
      expect(pushed).toContain('[Source: Local note] (/path/to/vault)')
      expect(pushed).toContain('hello world')
    })

    it('truncates the excerpt at 2000 characters', async () => {
      await queryWithItem({
        id: 'n3',
        title: 'Big Note',
        body: 'a'.repeat(2500),
        url: 'https://example.com/big',
        kind: 'markdown',
        updated_at: null,
      })

      await userEvent.setup().click(screen.getByRole('button', { name: /add to chat/i }))

      const pushed = mockPushComposerDraft.mock.calls[0][0]
      expect(pushed).toBe(`[Source: Big Note] (https://example.com/big)\n${'a'.repeat(2000)}`)
    })

    // G5 P0-6 — the regression this whole fix is about: the composer only
    // exists on /chat, so the push used to fire with nobody listening and
    // the draft was silently lost behind a success toast. Now the pending
    // queue holds it while the page navigates, and the composer flushes it
    // on mount.
    it('cross-route push: the draft lands in the composer after the navigate to /chat', async () => {
      const user = userEvent.setup()
      mockQueryDataSource.mockResolvedValue({
        items: [
          {
            id: 'n4',
            title: 'Test Note',
            body: 'hello cross-route',
            url: 'https://example.com/note',
            kind: 'markdown',
            updated_at: null,
          },
        ],
        total: 1,
        has_more: false,
      })
      renderQueryPage()

      await waitFor(() => {
        expect(screen.getByLabelText(/select data source/i)).toBeInTheDocument()
      })
      await user.selectOptions(screen.getByLabelText(/select data source/i), 'obsidian-vault')
      await user.type(screen.getByPlaceholderText(/enter your search query/i), 'test')
      await user.click(screen.getByRole('button', { name: /search/i }))
      await waitFor(() => {
        expect(screen.getByRole('button', { name: /add to chat/i })).toBeInTheDocument()
      })

      await user.click(screen.getByRole('button', { name: /add to chat/i }))

      // Navigation happened…
      await waitFor(() => {
        expect(screen.getByTestId('chat-page')).toBeInTheDocument()
      })
      // …and the queued draft flushed into the composer probe.
      await waitFor(() => {
        expect(screen.getAllByTestId('composer-draft')).toHaveLength(1)
      })
      expect(screen.getByTestId('composer-draft').textContent).toBe(
        '[Source: Test Note] (https://example.com/note)\nhello cross-route',
      )
      expect(mockToastSuccess).toHaveBeenCalledWith('Added to the chat draft')
    })
  })
})
