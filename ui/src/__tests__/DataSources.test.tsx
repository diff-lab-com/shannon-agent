import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor, fireEvent } from '@testing-library/react'
import { MemoryRouter, Routes, Route, Outlet } from 'react-router-dom'
import DataSources from '@/components/extensions/DataSources'

function Shell() {
  return <Outlet context={{ search: '' }} />
}

const listDataSourceCatalog = vi.hoisted(() => vi.fn())
const listInstalledDataSources = vi.hoisted(() => vi.fn())
const installDataSource = vi.hoisted(() => vi.fn())
const uninstallDataSource = vi.hoisted(() => vi.fn())

vi.mock('@/lib/tauri-api', () => ({
  default: {},
  listDataSourceCatalog: (...a: unknown[]) => listDataSourceCatalog(...a),
  listInstalledDataSources: (...a: unknown[]) => listInstalledDataSources(...a),
  installDataSource: (...a: unknown[]) => installDataSource(...a),
  uninstallDataSource: (...a: unknown[]) => uninstallDataSource(...a),
}))

function renderWithRouter() {
  return render(
    <MemoryRouter initialEntries={['/extensions/datasources']}>
      <Routes>
        <Route path="/*" element={<Shell />}>
          <Route path="*" element={<DataSources />} />
        </Route>
      </Routes>
    </MemoryRouter>
  )
}

const obsidianEntry = {
  id: 'native:data-source-obsidian-vault',
  kind: 'data_source' as const,
  name: 'Obsidian Vault',
  description: 'Read markdown notes from a local Obsidian vault.',
  author: 'Shannon',
  version: '0.2.4',
  homepage_url: 'https://obsidian.md',
  license: 'Apache-2.0',
  stars: null,
  last_updated: null,
  source: { type: 'native' as const },
  trust: 'verified' as const,
  metadata: {
    kind: 'obsidian',
    fields: [
      { key: 'vault_path', label: 'Vault path', kind: 'path', required: true, placeholder: '/home/user/MyVault', help: null },
    ],
  },
  tags: ['native', 'obsidian'],
}

const emailEntry = {
  id: 'native:data-source-email-imap',
  kind: 'data_source' as const,
  name: 'Email (IMAP)',
  description: 'Connect to an IMAP server to read mailbox messages.',
  author: 'Shannon',
  version: '0.2.4',
  homepage_url: null,
  license: 'Apache-2.0',
  stars: null,
  last_updated: null,
  source: { type: 'native' as const },
  trust: 'verified' as const,
  metadata: {
    kind: 'email_imap',
    fields: [
      { key: 'imap_host', label: 'IMAP host', kind: 'text', required: true, placeholder: 'imap.gmail.com' },
      { key: 'imap_port', label: 'IMAP port', kind: 'number', required: true, placeholder: '993' },
      { key: 'username', label: 'Username', kind: 'text', required: true, placeholder: 'you@example.com' },
      { key: 'password', label: 'Password / app password', kind: 'password', required: true, placeholder: null },
    ],
  },
  tags: ['native', 'email_imap'],
}

const installedObsidian = {
  slug: 'obsidian-vault',
  kind: 'obsidian',
  name: 'Obsidian Vault',
  path: '/home/user/.shannon/data-sources/obsidian-vault.toml',
  installed_at: '2026-06-15T00:00:00Z',
}

beforeEach(() => {
  listDataSourceCatalog.mockReset()
  listInstalledDataSources.mockReset()
  installDataSource.mockReset()
  uninstallDataSource.mockReset()
})

describe('DataSources (P5 native adapters)', () => {
  it('renders catalog and installed headers', async () => {
    listDataSourceCatalog.mockResolvedValue([])
    listInstalledDataSources.mockResolvedValue([])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText(/Adapters · 0/)).toBeInTheDocument()
    })
    expect(screen.getByText(/Installed · 0/)).toBeInTheDocument()
  })

  it('shows loading state for catalog', () => {
    listDataSourceCatalog.mockReturnValue(new Promise(() => {}))
    listInstalledDataSources.mockResolvedValue([])
    renderWithRouter()
    expect(screen.getByText('Loading adapters…')).toBeInTheDocument()
  })

  it('renders adapter cards with name and description', async () => {
    listDataSourceCatalog.mockResolvedValue([obsidianEntry, emailEntry])
    listInstalledDataSources.mockResolvedValue([])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('Obsidian Vault')).toBeInTheDocument()
    })
    expect(screen.getByText('Email (IMAP)')).toBeInTheDocument()
    expect(screen.getByText(/Read markdown notes/)).toBeInTheDocument()
  })

  it('shows Installed badge when adapter already installed', async () => {
    listDataSourceCatalog.mockResolvedValue([obsidianEntry])
    listInstalledDataSources.mockResolvedValue([installedObsidian])
    renderWithRouter()
    await waitFor(() => {
      const installedBadges = screen.getAllByText('Installed')
      expect(installedBadges.length).toBeGreaterThan(0)
    })
  })

  it('expands install form on Configure & Install click', async () => {
    listDataSourceCatalog.mockResolvedValue([obsidianEntry])
    listInstalledDataSources.mockResolvedValue([])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('Obsidian Vault')).toBeInTheDocument()
    })
    fireEvent.click(screen.getByText('Configure & Install'))
    expect(screen.getByText('Vault path')).toBeInTheDocument()
    expect(screen.getByPlaceholderText('/home/user/MyVault')).toBeInTheDocument()
    expect(screen.getByText('Save')).toBeInTheDocument()
  })

  it('validates required fields before submit', async () => {
    listDataSourceCatalog.mockResolvedValue([obsidianEntry])
    listInstalledDataSources.mockResolvedValue([])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('Obsidian Vault')).toBeInTheDocument()
    })
    fireEvent.click(screen.getByText('Configure & Install'))
    // Clear placeholder default
    fireEvent.change(screen.getByPlaceholderText('/home/user/MyVault'), { target: { value: '' } })
    fireEvent.click(screen.getByText('Save'))
    await waitFor(() => {
      expect(screen.getByText(/Vault path is required/)).toBeInTheDocument()
    })
    expect(installDataSource).not.toHaveBeenCalled()
  })

  it('submits install with form values', async () => {
    listDataSourceCatalog.mockResolvedValue([obsidianEntry])
    listInstalledDataSources.mockResolvedValue([])
    installDataSource.mockResolvedValue({
      id: 'native:data-source-obsidian-vault',
      name: 'Obsidian Vault',
      install_path: '/home/user/.shannon/data-sources/obsidian-vault.toml',
    })
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('Obsidian Vault')).toBeInTheDocument()
    })
    fireEvent.click(screen.getByText('Configure & Install'))
    fireEvent.change(screen.getByPlaceholderText('/home/user/MyVault'), {
      target: { value: '/home/me/Vault' },
    })
    fireEvent.click(screen.getByText('Save'))
    await waitFor(() => {
      expect(installDataSource).toHaveBeenCalled()
    })
    const args = installDataSource.mock.calls[0]
    expect(args[0]).toBe('obsidian-vault')
    expect(args[1]).toBe('obsidian')
    expect(args[2]).toBe('Obsidian Vault')
    expect(args[3]).toMatchObject({ vault_path: '/home/me/Vault' })
  })

  it('renders installed section with adapter slug and Remove button', async () => {
    listDataSourceCatalog.mockResolvedValue([])
    listInstalledDataSources.mockResolvedValue([installedObsidian])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText(/Installed · 1/)).toBeInTheDocument()
    })
    expect(screen.getByText('Obsidian Vault')).toBeInTheDocument()
    expect(screen.getByText(/obsidian-vault · obsidian/)).toBeInTheDocument()
    expect(screen.getByText('Remove')).toBeInTheDocument()
  })

  it('calls uninstallDataSource on Remove click', async () => {
    listDataSourceCatalog.mockResolvedValue([])
    listInstalledDataSources.mockResolvedValue([installedObsidian])
    uninstallDataSource.mockResolvedValue(undefined)
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('Remove')).toBeInTheDocument()
    })
    fireEvent.click(screen.getByText('Remove'))
    await waitFor(() => {
      expect(uninstallDataSource).toHaveBeenCalledWith('obsidian-vault')
    })
  })

  it('shows empty state when no adapters available', async () => {
    listDataSourceCatalog.mockResolvedValue([])
    listInstalledDataSources.mockResolvedValue([])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('No data source adapters found.')).toBeInTheDocument()
    })
  })
})
