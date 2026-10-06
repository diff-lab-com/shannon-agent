// X2 安装时信任卡 — the InstallDialog must show WHAT an install will enable
// before the user confirms, derived strictly from data the catalog entry
// carries (source, package metadata, description). Fields the manifest does
// not model (MCP tool lists, runtime permission categories) must NOT be
// invented — the after-install fallback line covers them instead.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import InstallDialog from '@/components/extensions/InstallDialog'
import type { CatalogEntry } from '@/types'
import type * as api from '@/lib/tauri-api'
// Hoisted static import of the mocked fns (vi.mock lifts the factory above
// imports, so these are the mock doubles).
import { inspectPluginSource, installPluginFromGit } from '@/lib/tauri-api'

// X5 bundle-preview tests control `inspectPluginSource` directly; everything
// else keeps hitting the globally-mocked invoke via the real wrappers.
vi.mock('@/lib/tauri-api', async importOriginal => {
  const actual = await importOriginal<typeof api>()
  return {
    ...actual,
    inspectPluginSource: vi.fn(),
    installPluginFromGit: vi.fn(),
  }
})

const entry = (overrides: Partial<CatalogEntry> = {}): CatalogEntry => ({
  id: 'gh:test/plugin',
  kind: 'skill',
  name: 'Test Plugin',
  description: 'A test plugin for verification.',
  author: 'Test Author',
  version: '1.0.0',
  homepage_url: null,
  license: 'MIT',
  stars: 0,
  last_updated: null,
  source: { type: 'git_hub_repo', repo: 'test/plugin', ref_: 'main' },
  trust: 'community',
  metadata: {},
  tags: [],
  ...overrides,
})

function renderDialog(spec: CatalogEntry) {
  return render(
    <MemoryRouter>
      <InstallDialog entry={spec} open onClose={vi.fn()} onInstalled={vi.fn()} />
    </MemoryRouter>,
  )
}

describe('InstallDialog — X5 plugin bundle preview', () => {
  beforeEach(() => {
    vi.mocked(inspectPluginSource).mockReset()
    vi.mocked(installPluginFromGit).mockReset()
  })

  const pluginEntry = entry({
    kind: 'plugin',
    name: 'Starter Pack',
    description: 'A full plugin bundle.',
    source: { type: 'git_hub_repo', repo: 'shannon-agent/shannon-starter', ref_: 'main' },
    metadata: { marketplace_manifest: 'https://github.com/shannon-agent/shannon-starter/raw/main/.claude-plugin/marketplace.json' },
  })

  it('fetches inspect_plugin_source and renders the bundle checklist before install', async () => {
    vi.mocked(inspectPluginSource).mockResolvedValue({
      name: 'shannon-starter',
      source_format: 'claude-json',
      skills: ['brainstorm', 'tdd'],
      agents: ['reviewer.md'],
      commands: ['ship.md'],
      mcp_servers: ['filesystem'],
    })
    const { fireEvent, waitFor } = await import('@testing-library/react')
    renderDialog(pluginEntry)

    // the inspect call carries the git URL derived from the card source
    await waitFor(() =>
      expect(inspectPluginSource).toHaveBeenCalledWith('https://github.com/shannon-agent/shannon-starter.git'),
    )
    await waitFor(() => expect(screen.getByTestId('plugin-bundle-card')).toBeInTheDocument())
    expect(screen.getByTestId('bundle-skills')).toHaveTextContent('2 skills: brainstorm, tdd')
    expect(screen.getByTestId('bundle-agents')).toHaveTextContent('1 agents: reviewer.md')
    expect(screen.getByTestId('bundle-commands')).toHaveTextContent('1 commands: ship.md')
    expect(screen.getByTestId('bundle-mcp')).toHaveTextContent('MCP servers: filesystem')

    // confirm still goes through installPluginFromGit with the default
    // (no opt-in) consent
    expect(screen.getByRole('button', { name: /Install & authorize/ })).toBeEnabled()
    fireEvent.click(screen.getByRole('button', { name: /Install & authorize/ }))
    await waitFor(() =>
      expect(installPluginFromGit).toHaveBeenCalledWith(
        'https://github.com/shannon-agent/shannon-starter.git',
        false,
      ),
    )
  })

  it('disables install while the preview is loading and on preview failure', async () => {
    vi.mocked(inspectPluginSource).mockRejectedValue(new Error('clone boom'))
    renderDialog(pluginEntry)
    const { waitFor } = await import('@testing-library/react')
    await waitFor(() => expect(screen.getByTestId('bundle-error')).toBeInTheDocument())
    expect(screen.getByRole('button', { name: /Install & authorize/ })).toBeDisabled()
    expect(inspectPluginSource).toHaveBeenCalled()
  })

  it('offers the explicit unverified opt-in after an SEC-1 refusal', async () => {
    vi.mocked(inspectPluginSource).mockResolvedValue({
      name: 'shady',
      source_format: 'claude-json',
      skills: [],
      agents: [],
      commands: [],
      mcp_servers: [],
    })
    vi.mocked(installPluginFromGit).mockRejectedValue(
      new Error('remote plugin "shady" declares no permissions; pass allow_unverified to install anyway'),
    )
    const { fireEvent, waitFor } = await import('@testing-library/react')
    renderDialog(pluginEntry)
    const installBtn = await waitFor(() => {
      const btn = screen.getByRole('button', { name: /Install & authorize/ })
      expect(btn).toBeEnabled()
      return btn
    })
    fireEvent.click(installBtn)
    const anyway = await screen.findByTestId('install-unverified')
    expect(screen.getByTestId('unverified-warning')).toBeInTheDocument()

    // the explicit opt-in retries with allowUnverified: true
    vi.mocked(installPluginFromGit).mockResolvedValue({ name: 'shady', warnings: [] })
    fireEvent.click(anyway)
    await waitFor(() =>
      expect(installPluginFromGit).toHaveBeenLastCalledWith(
        'https://github.com/shannon-agent/shannon-starter.git',
        true,
      ),
    )
  })

  it('shows the empty-bundle note when the source carries nothing', async () => {
    vi.mocked(inspectPluginSource).mockResolvedValue({
      name: 'empty',
      source_format: 'shannon-toml',
      skills: [],
      agents: [],
      commands: [],
      mcp_servers: [],
    })
    renderDialog(pluginEntry)
    const { waitFor } = await import('@testing-library/react')
    await waitFor(() => expect(screen.getByTestId('bundle-empty')).toBeInTheDocument())
  })
})

describe('InstallDialog — X2 trust card', () => {
  it('shows the will-be-enabled card with registry source + local-command capability for an mcp_registry entry', () => {
    renderDialog(
      entry({
        kind: 'mcp',
        name: 'github-mcp',
        description: '',
        source: { type: 'mcp_registry', publisher: 'acme' },
        metadata: { package: { type: 'npm', name: '@acme/github-mcp' } },
      }),
    )
    const card = screen.getByTestId('install-trust-card')
    expect(card).toBeInTheDocument()
    expect(screen.getByText('Will be enabled')).toBeInTheDocument()
    expect(screen.getByText(/MCP registry · publisher acme/)).toBeInTheDocument()
    expect(screen.getByTestId('trust-capability-command')).toHaveTextContent(
      'Runs a local command',
    )
    // Install-time network fetch is derivable from a remote registry source.
    expect(screen.getByText('Fetches from the network at install time')).toBeInTheDocument()
    // Fallback for unmodelled fields (tool list): the after-install pointer.
    expect(
      screen.getByText(/Details such as tool lists appear in the Installed tab/),
    ).toBeInTheDocument()
    // X2: confirm button carries authorization wording.
    expect(screen.getByRole('button', { name: /Install & authorize/ })).toBeEnabled()
  })

  it('shows container capability for docker packages', () => {
    renderDialog(
      entry({
        kind: 'mcp',
        name: 'docker-mcp',
        description: '',
        source: { type: 'mcp_registry', publisher: 'acme' },
        metadata: { package: { type: 'docker', name: 'acme/mcp:latest' } },
      }),
    )
    expect(screen.getByText('Runs a local command')).toBeInTheDocument()
    expect(screen.getByText('Runs in a Docker container')).toBeInTheDocument()
  })

  it('shows the GitHub source and the skill trigger description for a git_hub_repo skill', () => {
    renderDialog(
      entry({
        kind: 'skill',
        name: 'Deploy Helper',
        description: 'Runs the deploy checklist when asked.',
        source: { type: 'git_hub_repo', repo: 'test/skill', ref_: 'main' },
      }),
    )
    expect(screen.getByText('GitHub repository')).toBeInTheDocument()
    // The repo appears in both the trust card and the GitHub body.
    expect(screen.getAllByText('test/skill').length).toBeGreaterThan(0)
    expect(screen.getByText('Trigger description')).toBeInTheDocument()
    expect(
      screen.getAllByText(/Runs the deploy checklist when asked\./).length,
    ).toBeGreaterThan(0)
    expect(screen.getByRole('button', { name: /Install & authorize/ })).toBeInTheDocument()
  })

  it('lists OAuth scope capability for featured vendors', () => {
    renderDialog(
      entry({
        kind: 'mcp',
        name: 'vendor-mcp',
        description: '',
        source: { type: 'featured_vendor' },
        metadata: {
          transport: 'oauth_remote',
          vendor: 'Anthropic',
          endpoint: 'https://example.com/oauth',
          scopes: ['mcp:read', 'mcp:write'],
        },
      }),
    )
    expect(screen.getByText(/Featured vendor · Anthropic/)).toBeInTheDocument()
    expect(screen.getByText('OAuth authorization scopes')).toBeInTheDocument()
  })

  it('stays honest for a native entry: local-config source, no install button', () => {
    renderDialog(
      entry({
        kind: 'plugin',
        name: 'Native Thing',
        description: '',
        source: { type: 'native' },
      }),
    )
    expect(screen.getByText('Local configuration')).toBeInTheDocument()
    // Fallback body routes to the dedicated tab — nothing installs here, so
    // there must be no "Install & authorize" confirm button.
    expect(screen.queryByRole('button', { name: /Install & authorize/ })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: /Open tab/ })).toBeInTheDocument()
  })
})
