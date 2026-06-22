import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import DataSourcesQuery from '@/components/extensions/DataSourcesQuery'
import * as api from '@/lib/tauri-api'

// Mock the tauri-api module
vi.mock('@/lib/tauri-api')

describe('DataSourcesQuery', () => {
  const mockListInstalledDataSources = vi.mocked(api.listInstalledDataSources)
  const mockQueryDataSource = vi.mocked(api.queryDataSource)

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

      await waitFor(() => {
        expect(screen.getByText(/no results found/i)).toBeInTheDocument()
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
})
