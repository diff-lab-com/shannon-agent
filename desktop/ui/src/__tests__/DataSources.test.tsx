import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor, fireEvent, within } from '@testing-library/react'
import { MemoryRouter, Routes, Route, Outlet } from 'react-router-dom'
import DataSources from '@/components/extensions/DataSources'

function Shell() {
  return <Outlet context={{ search: '' }} />
}

const listDataSourceCatalog = vi.hoisted(() => vi.fn())
const listInstalledDataSources = vi.hoisted(() => vi.fn())
const installDataSource = vi.hoisted(() => vi.fn())
const uninstallDataSource = vi.hoisted(() => vi.fn())
const queryDataSource = vi.hoisted(() => vi.fn())
// Office Wave 2 B3 — DataSources renders DataSourcesQuery (query tab), which
// imports the composer bridge + sonner; mock both so the module graph stays
// hermetic (and so the bridge file's merge state never affects these tests).
const pushComposerDraft = vi.hoisted(() => vi.fn())
const toastSuccess = vi.hoisted(() => vi.fn())
const toastError = vi.hoisted(() => vi.fn())

vi.mock('@/lib/tauri-api', () => ({
  default: {},
  listDataSourceCatalog: (...a: unknown[]) => listDataSourceCatalog(...a),
  listInstalledDataSources: (...a: unknown[]) => listInstalledDataSources(...a),
  installDataSource: (...a: unknown[]) => installDataSource(...a),
  uninstallDataSource: (...a: unknown[]) => uninstallDataSource(...a),
  queryDataSource: (...a: unknown[]) => queryDataSource(...a),
}))

vi.mock('@/lib/composerBridge', () => ({
  pushComposerDraft: (...a: unknown[]) => pushComposerDraft(...a),
}))

vi.mock('sonner', () => ({
  toast: {
    success: (...a: unknown[]) => toastSuccess(...a),
    error: (...a: unknown[]) => toastError(...a),
  },
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
  queryDataSource.mockReset()
  pushComposerDraft.mockReset()
  toastSuccess.mockReset()
  toastError.mockReset()
})

// F5 (A8): an installed row whose credentials live in the OS keyring.
const installedKeyringSource = {
  slug: 'imap-home',
  kind: 'email_imap',
  name: 'Home',
  path: '/home/user/.shannon/data-sources/imap-home.toml',
  installed_at: '2026-06-15T00:00:00Z',
  credential_storage: 'keyring',
}

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

  // Office Wave 2 B3 — the Rust dispatch now ships real fetchers for
  // obsidian/email_imap, so Wave 1 A4's honesty badges flip back:
  // both kinds wear Verified again and never show the coming-soon /
  // query-in-development copy.
  it('shows the Verified badge for uninstalled obsidian now that a fetcher ships', async () => {
    listDataSourceCatalog.mockResolvedValue([obsidianEntry])
    listInstalledDataSources.mockResolvedValue([])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('Obsidian Vault')).toBeInTheDocument()
    })
    expect(screen.getByText('Verified')).toBeInTheDocument()
    expect(screen.queryByText('Query coming soon')).not.toBeInTheDocument()
    expect(screen.queryByText('Configured · query in development')).not.toBeInTheDocument()
  })

  it('shows Verified for installed obsidian', async () => {
    listDataSourceCatalog.mockResolvedValue([obsidianEntry])
    listInstalledDataSources.mockResolvedValue([installedObsidian])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('Verified')).toBeInTheDocument()
    })
    expect(screen.queryByText('Configured · query in development')).not.toBeInTheDocument()
  })

  it('shows Verified for installed email_imap', async () => {
    listDataSourceCatalog.mockResolvedValue([emailEntry])
    listInstalledDataSources.mockResolvedValue([
      { ...installedObsidian, slug: 'email-imap', kind: 'email_imap', name: 'Email (IMAP)' },
    ])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('Verified')).toBeInTheDocument()
    })
    expect(screen.queryByText('Configured · query in development')).not.toBeInTheDocument()
  })

  // Wave 2 keeps the coming-soon family for kinds whose query path is still
  // config-only (slack/discord/telegram/rss/ical).
  it('keeps the coming-soon badge for config-only kinds (slack)', async () => {
    const slackEntry = {
      ...obsidianEntry,
      id: 'native:data-source-slack',
      name: 'Slack',
      metadata: { kind: 'slack', fields: [] },
    }
    listDataSourceCatalog.mockResolvedValue([slackEntry])
    listInstalledDataSources.mockResolvedValue([])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('Slack')).toBeInTheDocument()
    })
    expect(screen.getByText('Query coming soon')).toBeInTheDocument()
    expect(screen.queryByText('Verified')).not.toBeInTheDocument()
  })

  it('keeps the Verified badge for adapters with a real query fetcher (notion)', async () => {
    const notionEntry = {
      ...obsidianEntry,
      id: 'native:data-source-notion',
      name: 'Notion',
      metadata: { kind: 'notion', fields: [] },
    }
    listDataSourceCatalog.mockResolvedValue([notionEntry])
    listInstalledDataSources.mockResolvedValue([])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('Notion')).toBeInTheDocument()
    })
    expect(screen.getByText('Verified')).toBeInTheDocument()
    expect(screen.queryByText('Query coming soon')).not.toBeInTheDocument()
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

  // F5 (A8): the credential-storage status line — OS keychain mode.
  it('shows the OS-keychain credential-storage line when sources migrated', async () => {
    listDataSourceCatalog.mockResolvedValue([emailEntry])
    listInstalledDataSources.mockResolvedValue([installedKeyringSource])
    renderWithRouter()
    const line = await screen.findByTestId('datasources-credential-storage')
    expect(line).toHaveTextContent('Credential storage')
    expect(line).toHaveTextContent('OS keychain')
  })

  // F5 (A8): the honest degraded mode — keyring unavailable.
  it('shows the local-file credential-storage line when degraded', async () => {
    listDataSourceCatalog.mockResolvedValue([emailEntry])
    listInstalledDataSources.mockResolvedValue([
      { ...installedKeyringSource, credential_storage: 'plaintext_file' },
    ])
    renderWithRouter()
    const line = await screen.findByTestId('datasources-credential-storage')
    expect(line).toHaveTextContent('Local file (restricted)')
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
    const dialog = await screen.findByRole('alertdialog', { name: /Remove data source\?/i })
    fireEvent.click(within(dialog).getByRole('button', { name: /^Remove$/ }))
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

// Office Wave 2 B3 — "Fetch now" on installed obsidian/email_imap rows runs
// the same command as the query panel (query_data_source) with an empty
// query (the fetchers treat empty as "list everything"), then toasts the
// result count.
describe('DataSources — Fetch now (Office Wave 2 B3)', () => {
  it('shows Fetch now on installed obsidian/email_imap rows only', async () => {
    listDataSourceCatalog.mockResolvedValue([])
    listInstalledDataSources.mockResolvedValue([
      installedObsidian,
      { ...installedObsidian, slug: 'email-imap', kind: 'email_imap', name: 'Email (IMAP)' },
      { ...installedObsidian, slug: 'my-slack', kind: 'slack', name: 'Slack bridge' },
    ])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByTestId('fetch-now-obsidian-vault')).toBeInTheDocument()
    })
    expect(screen.getByTestId('fetch-now-email-imap')).toBeInTheDocument()
    expect(screen.getAllByText('Fetch now')).toHaveLength(2)
    expect(screen.queryByTestId('fetch-now-my-slack')).not.toBeInTheDocument()
  })

  it('Fetch now triggers the query command and toasts the result count', async () => {
    listDataSourceCatalog.mockResolvedValue([])
    listInstalledDataSources.mockResolvedValue([installedObsidian])
    queryDataSource.mockResolvedValue({ items: [], total: 2, has_more: false })
    renderWithRouter()
    fireEvent.click(await screen.findByTestId('fetch-now-obsidian-vault'))
    await waitFor(() => {
      expect(queryDataSource).toHaveBeenCalledWith('obsidian-vault', '')
    })
    await waitFor(() => {
      expect(toastSuccess).toHaveBeenCalledWith(expect.stringContaining('2 results'))
    })
  })

  it('Fetch now failures toast the error', async () => {
    listDataSourceCatalog.mockResolvedValue([])
    listInstalledDataSources.mockResolvedValue([installedObsidian])
    queryDataSource.mockRejectedValue(new Error('vault path missing'))
    renderWithRouter()
    fireEvent.click(await screen.findByTestId('fetch-now-obsidian-vault'))
    await waitFor(() => {
      expect(toastError).toHaveBeenCalledWith('Query failed', expect.objectContaining({ description: 'vault path missing' }))
    })
  })
})
