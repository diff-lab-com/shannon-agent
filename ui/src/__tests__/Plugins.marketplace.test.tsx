import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor, fireEvent } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import Plugins from '@/components/extensions/Plugins'
import type { CatalogEntry } from '@/types'
import * as api from '@/lib/tauri-api'

vi.mock('react-router-dom', async () => {
  const actual = await vi.importActual<typeof import('react-router-dom')>('react-router-dom')
  return {
    ...actual,
    useOutletContext: () => ({ search: (globalThis as { __PLUGINS_SEARCH__?: string }).__PLUGINS_SEARCH__ ?? '' }),
    useNavigate: () => () => {},
  }
})

const entry = (overrides: Partial<CatalogEntry> = {}): CatalogEntry => ({
  id: 'gh:test/plugin',
  kind: 'skill',
  name: 'Test Plugin',
  description: 'A test plugin for verification.',
  author: 'Test Author',
  version: '1.0.0',
  homepage_url: 'https://example.com/plugin',
  license: 'MIT',
  stars: 1200,
  last_updated: null,
  source: { type: 'git_hub_repo', repo: 'test/plugin', ref_: 'main' },
  trust: 'community',
  metadata: {},
  tags: ['test'],
  ...overrides,
})

function renderPlugins() {
  return render(<Plugins />, { wrapper: I18nProvider })
}

describe('Plugins (marketplace browser)', () => {
  beforeEach(() => {
    vi.mocked(api.listPluginMarketplace).mockReset()
    vi.mocked(api.saveTextFile).mockReset()
    vi.mocked(api.installSkillFromRepo).mockReset()
    vi.mocked(api.installAgentFromRepo).mockReset()
    ;(globalThis as { __PLUGINS_SEARCH__?: string }).__PLUGINS_SEARCH__ = ''
  })

  it('renders entries after the catalog resolves', async () => {
    vi.mocked(api.listPluginMarketplace).mockResolvedValue([
      entry({ id: 'a', name: 'Alpha Skill' }),
      entry({ id: 'b', kind: 'mcp', name: 'Beta MCP', trust: 'official' }),
    ])
    renderPlugins()
    await waitFor(() => expect(screen.getByText('Alpha Skill')).toBeInTheDocument())
    expect(screen.getByText('Beta MCP')).toBeInTheDocument()
  })

  it('shows empty state when catalog returns []', async () => {
    vi.mocked(api.listPluginMarketplace).mockResolvedValue([])
    renderPlugins()
    await waitFor(() => expect(screen.getByText(/No catalog entries available/i)).toBeInTheDocument())
  })

  it('shows error state when the fetch rejects', async () => {
    vi.mocked(api.listPluginMarketplace).mockRejectedValue(new Error('network'))
    renderPlugins()
    await waitFor(() => expect(screen.getByText(/Failed to load catalog/i)).toBeInTheDocument())
  })

  it('filters by kind when a kind chip is clicked', async () => {
    vi.mocked(api.listPluginMarketplace).mockResolvedValue([
      entry({ id: 'a', kind: 'skill', name: 'Skill One' }),
      entry({ id: 'b', kind: 'mcp', name: 'MCP One' }),
    ])
    renderPlugins()
    await waitFor(() => expect(screen.getByText('Skill One')).toBeInTheDocument())
    const mcpChip = screen.getAllByText('MCP Servers').find((el) => el.closest('button') !== null)!
    fireEvent.click(mcpChip)
    await waitFor(() => expect(screen.queryByText('Skill One')).not.toBeInTheDocument())
    expect(screen.getByText('MCP One')).toBeInTheDocument()
  })

  it('groups rows under their kind heading', async () => {
    vi.mocked(api.listPluginMarketplace).mockResolvedValue([
      entry({ id: 'a', kind: 'mcp', name: 'Mcp A' }),
      entry({ id: 'b', kind: 'skill', name: 'Skill B' }),
    ])
    renderPlugins()
    await waitFor(() => expect(screen.getByText('Mcp A')).toBeInTheDocument())
    expect(screen.getAllByText('Skills').length).toBeGreaterThan(0)
    expect(screen.getAllByText('MCP Servers').length).toBeGreaterThan(0)
  })

  it('renders the correct trust badge per level', async () => {
    vi.mocked(api.listPluginMarketplace).mockResolvedValue([
      entry({ id: 'a', name: 'Verified One', trust: 'verified' }),
      entry({ id: 'b', name: 'Community Two', trust: 'community' }),
    ])
    renderPlugins()
    await waitFor(() => expect(screen.getByText('Verified One')).toBeInTheDocument())
    expect(screen.getAllByText('Verified').length).toBeGreaterThan(0)
    expect(screen.getAllByText('Community').length).toBeGreaterThan(0)
  })

  it('renders license, version, and stars (compact for thousands)', async () => {
    vi.mocked(api.listPluginMarketplace).mockResolvedValue([
      entry({ name: 'WithMeta', license: 'Apache-2.0', version: '2.3.1', stars: 1500 }),
    ])
    renderPlugins()
    await waitFor(() => expect(screen.getByText('WithMeta')).toBeInTheDocument())
    expect(screen.getByText('Apache-2.0')).toBeInTheDocument()
    expect(screen.getByText('2.3.1')).toBeInTheDocument()
    expect(screen.getByText('1.5k')).toBeInTheDocument()
  })

  it('renders homepage link when homepage_url is set', async () => {
    vi.mocked(api.listPluginMarketplace).mockResolvedValue([
      entry({ name: 'Linked', homepage_url: 'https://example.com/x' }),
    ])
    renderPlugins()
    await waitFor(() => expect(screen.getByText('Linked')).toBeInTheDocument())
    expect(screen.getByText('Homepage').getAttribute('href')).toBe('https://example.com/x')
  })

  it('filters rows by search query via outlet context', async () => {
    ;(globalThis as { __PLUGINS_SEARCH__?: string }).__PLUGINS_SEARCH__ = 'notion'
    vi.mocked(api.listPluginMarketplace).mockResolvedValue([
      entry({ id: 'a', name: 'Notion Sync', tags: ['notion'] }),
      entry({ id: 'b', name: 'Unrelated Tool' }),
    ])
    renderPlugins()
    await waitFor(() => expect(screen.getByText('Notion Sync')).toBeInTheDocument())
    expect(screen.queryByText('Unrelated Tool')).not.toBeInTheDocument()
  })

  it('sorts entries by stars in descending order', async () => {
    vi.mocked(api.listPluginMarketplace).mockResolvedValue([
      entry({ id: 'a', name: 'Low Stars', stars: 100 }),
      entry({ id: 'b', name: 'High Stars', stars: 5000 }),
      entry({ id: 'c', name: 'Medium Stars', stars: 1000 }),
    ])
    renderPlugins()
    await waitFor(() => expect(screen.getByText('Low Stars')).toBeInTheDocument())

    // Find the sort dropdown by label text
    const sortLabel = screen.getByText('Sort by')
    const sortSelect = sortLabel.nextElementSibling as HTMLSelectElement
    expect(sortSelect).toBeInTheDocument()

    // Change to stars sort
    fireEvent.change(sortSelect, { target: { value: 'stars' } })

    // After sorting by stars (5000 > 1000 > 100), High Stars should come before Medium Stars before Low Stars
    await waitFor(() => {
      const allText = document.body.textContent || ''
      const highStarsIndex = allText.indexOf('High Stars')
      const mediumStarsIndex = allText.indexOf('Medium Stars')
      const lowStarsIndex = allText.indexOf('Low Stars')

      expect(highStarsIndex).toBeGreaterThan(-1)
      expect(mediumStarsIndex).toBeGreaterThan(-1)
      expect(lowStarsIndex).toBeGreaterThan(-1)

      // Verify ordering: high stars should appear first
      expect(highStarsIndex).toBeLessThan(mediumStarsIndex)
      expect(mediumStarsIndex).toBeLessThan(lowStarsIndex)
    })
  })

  it('calls installSkillFromRepo when installing a skill with git_hub_repo source', async () => {
    const skillEntry = entry({
      id: 'skill-1',
      kind: 'skill',
      name: 'Test Skill',
      source: { type: 'git_hub_repo', repo: 'test/skill', ref_: 'main' }
    })
    vi.mocked(api.listPluginMarketplace).mockResolvedValue([skillEntry])
    vi.mocked(api.installSkillFromRepo).mockResolvedValue({
      id: 'skill-1',
      name: 'Test Skill',
      install_path: '/path/to/skill'
    })

    renderPlugins()
    await waitFor(() => expect(screen.getByText('Test Skill')).toBeInTheDocument())

    // Click the card's Install button — opens the InstallDialog.
    fireEvent.click(screen.getByText('Install'))

    // The dialog shows the repo; click its Install button (the last one in the DOM).
    await waitFor(() => expect(screen.getByText('test/skill')).toBeInTheDocument())
    fireEvent.click(screen.getAllByText('Install').at(-1)!)

    await waitFor(() => {
      expect(api.installSkillFromRepo).toHaveBeenCalledWith('Test Skill', 'test/skill', 'main')
    })
  })

  it('dispatches shannon:extension-installed event after successful install', async () => {
    const eventSpy = vi.fn()
    window.addEventListener('shannon:extension-installed', eventSpy)

    const skillEntry = entry({
      id: 'skill-1',
      kind: 'skill',
      name: 'Test Skill',
      source: { type: 'git_hub_repo', repo: 'test/skill', ref_: 'main' }
    })
    vi.mocked(api.listPluginMarketplace).mockResolvedValue([skillEntry])
    vi.mocked(api.installSkillFromRepo).mockResolvedValue({
      id: 'skill-1',
      name: 'Test Skill',
      install_path: '/path/to/skill'
    })

    renderPlugins()
    await waitFor(() => expect(screen.getByText('Test Skill')).toBeInTheDocument())

    // Open the dialog, then click through to install.
    fireEvent.click(screen.getByText('Install'))
    await waitFor(() => expect(screen.getByText('test/skill')).toBeInTheDocument())
    fireEvent.click(screen.getAllByText('Install').at(-1)!)

    await waitFor(() => {
      expect(eventSpy).toHaveBeenCalledTimes(1)
      const event = eventSpy.mock.calls[0][0] as CustomEvent
      expect(event.detail.kind).toBe('skill')
      expect(event.detail.name).toBe('Test Skill')
    })

    window.removeEventListener('shannon:extension-installed', eventSpy)
  })
})
