import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import DataSourcesQuery from '@/components/extensions/DataSourcesQuery'
import * as api from '@/lib/tauri-api'
import { pushComposerDraft } from '@/lib/composerBridge'
import { toast } from 'sonner'

// Mock the tauri-api module
vi.mock('@/lib/tauri-api')

// Office Wave 2 B3 — the composer bridge (window CustomEvent
// 'shannon:composer-draft') is a sibling deliverable on this branch; mocking
// it here keeps these tests hermetic either way.
vi.mock('@/lib/composerBridge', () => ({
  pushComposerDraft: vi.fn(),
}))

vi.mock('sonner', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}))

describe('DataSourcesQuery', () => {
  const mockListInstalledDataSources = vi.mocked(api.listInstalledDataSources)
  const mockQueryDataSource = vi.mocked(api.queryDataSource)
  const mockPushComposerDraft = vi.mocked(pushComposerDraft)
  const mockToastSuccess = vi.mocked(toast.success)

  beforeEach(() => {
    vi.clearAllMocks()
  })

  describe('Loading state', () => {
    it('shows loading spinner while fetching installed data sources', async () => {
      mockListInstalledDataSources.mockImplementation(
        () => new Promise(() => {}) // Never resolves
      )

      render(<DataSourcesQuery />)

      expect(screen.getByText(/loading installed data sources/i)).toBeInTheDocument()
    })
  })

  describe('Empty state', () => {
    it('shows empty state when no data sources are installed', async () => {
      mockListInstalledDataSources.mockResolvedValue([])

      render(<DataSourcesQuery />)

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
      render(<DataSourcesQuery />)

      await waitFor(() => {
        expect(screen.getByLabelText(/select data source/i)).toBeInTheDocument()
        expect(screen.getByPlaceholderText(/enter your search query/i)).toBeInTheDocument()
        expect(screen.getByRole('button', { name: /search/i })).toBeInTheDocument()
      })
    })

    it('populates data source dropdown', async () => {
      render(<DataSourcesQuery />)

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
      render(<DataSourcesQuery />)

      await waitFor(() => {
        expect(screen.getByRole('button', { name: /search/i })).toBeInTheDocument()
      })

      const searchButton = screen.getByRole('button', { name: /search/i })
      expect(searchButton).toBeDisabled()
    })

    it('enables search button when form is complete', async () => {
      const user = userEvent.setup()
      render(<DataSourcesQuery />)

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

      render(<DataSourcesQuery />)

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

      render(<DataSourcesQuery />)

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

      render(<DataSourcesQuery />)

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

      render(<DataSourcesQuery />)

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

      render(<DataSourcesQuery />)

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

      render(<DataSourcesQuery />)

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
      render(<DataSourcesQuery />)

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
  })
})
