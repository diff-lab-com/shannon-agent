import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor, fireEvent, within } from '@testing-library/react'
import { MemoryRouter, Routes, Route, Outlet } from 'react-router-dom'
import Agents from '@/components/extensions/Agents'

function Shell() {
  return <Outlet context={{ search: '' }} />
}

const listAgentCatalog = vi.hoisted(() => vi.fn())
const listInstalledAgentPlugins = vi.hoisted(() => vi.fn())
const installNativeAgent = vi.hoisted(() => vi.fn())
const installAgentFromRepo = vi.hoisted(() => vi.fn())
const uninstallAgentPlugin = vi.hoisted(() => vi.fn())
const scanPromptInjectionWithReadme = vi.hoisted(() => vi.fn())

vi.mock('@/lib/tauri-api', () => ({
  default: {},
  listAgentCatalog: (...a: unknown[]) => listAgentCatalog(...a),
  listInstalledAgentPlugins: (...a: unknown[]) => listInstalledAgentPlugins(...a),
  installNativeAgent: (...a: unknown[]) => installNativeAgent(...a),
  installAgentFromRepo: (...a: unknown[]) => installAgentFromRepo(...a),
  uninstallAgentPlugin: (...a: unknown[]) => uninstallAgentPlugin(...a),
  scanPromptInjectionWithReadme: (...a: unknown[]) => scanPromptInjectionWithReadme(...a),
}))

function renderWithRouter() {
  return render(
    <MemoryRouter initialEntries={['/extensions/agents']}>
      <Routes>
        <Route path="/*" element={<Shell />}>
          <Route path="*" element={<Agents />} />
        </Route>
      </Routes>
    </MemoryRouter>
  )
}

const nativeAgent = {
  id: 'native:agent-code-reviewer',
  kind: 'agent' as const,
  name: 'code-reviewer',
  description: 'Reviews code for bugs, security issues, and best practices.',
  author: 'Shannon',
  version: '0.1.0',
  homepage_url: null,
  license: 'Apache-2.0',
  stars: null,
  last_updated: null,
  source: { type: 'native' as const },
  trust: 'verified' as const,
  metadata: {
    model: 'claude-sonnet-4-6',
    tools: ['read', 'grep', 'glob'],
    // G1 P1-9: native entries carry a system_prompt that the install maps
    // into the flat AgentDefinition TOML.
    system_prompt: 'You are a code reviewer.',
  },
  tags: ['code', 'review', 'native'],
}

const repoAgent = {
  id: 'gh:VoltAgent/awesome-claude-code-agents/main/doc-writer',
  kind: 'agent' as const,
  name: 'doc-writer',
  description: 'Technical doc writer agent.',
  author: 'VoltAgent/awesome-claude-code-agents',
  version: '1.0.0',
  homepage_url: 'https://github.com/VoltAgent/awesome-claude-code-agents',
  license: null,
  stars: null,
  last_updated: null,
  source: {
    type: 'git_hub_repo' as const,
    repo: 'VoltAgent/awesome-claude-code-agents',
    ref_: 'main',
  },
  trust: 'community' as const,
  metadata: {},
  tags: ['docs'],
}

const installedAgent = {
  name: 'code-reviewer',
  path: '/home/user/.shannon/agents/code-reviewer/agent.md',
  installed_at: '2026-06-15T00:00:00Z',
}

beforeEach(() => {
  listAgentCatalog.mockReset()
  listInstalledAgentPlugins.mockReset()
  installNativeAgent.mockReset()
  installAgentFromRepo.mockReset()
  uninstallAgentPlugin.mockReset()
  scanPromptInjectionWithReadme.mockReset()
  scanPromptInjectionWithReadme.mockResolvedValue({ risk: 'clean', matches: [], match_count: 0 })
})

describe('Agents (P4 federated catalog)', () => {
  it('renders catalog and installed headers', async () => {
    listAgentCatalog.mockResolvedValue([])
    listInstalledAgentPlugins.mockResolvedValue([])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText(/Catalog · 0/)).toBeInTheDocument()
    })
    expect(screen.getByText(/Installed · 0/)).toBeInTheDocument()
  })

  it('shows loading state for catalog', () => {
    listAgentCatalog.mockReturnValue(new Promise(() => {}))
    listInstalledAgentPlugins.mockResolvedValue([])
    renderWithRouter()
    expect(screen.getByText('Fetching agent catalog…')).toBeInTheDocument()
  })

  it('shows error state when catalog fetch fails', async () => {
    listAgentCatalog.mockRejectedValue(new Error('Network down'))
    listInstalledAgentPlugins.mockResolvedValue([])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText(/Failed to load catalog:/)).toBeInTheDocument()
    })
    expect(screen.getByText(/Network down/)).toBeInTheDocument()
  })

  it('renders catalog entries with trust badges and metadata', async () => {
    listAgentCatalog.mockResolvedValue([nativeAgent, repoAgent])
    listInstalledAgentPlugins.mockResolvedValue([])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('code-reviewer')).toBeInTheDocument()
    })
    expect(screen.getByText('doc-writer')).toBeInTheDocument()
    expect(screen.getByText('Verified')).toBeInTheDocument()
    expect(screen.getByText('Community')).toBeInTheDocument()
    // B6-36: the meta prefixes resolve through the locale files now
    // (extensions.myAgents.modelLabel / .toolsInline — en renders
    // "Model: …" / "Tools: …").
    expect(screen.getByText(/Model: claude-sonnet-4-6/)).toBeInTheDocument()
    expect(screen.getByText(/Tools: read, grep, glob/)).toBeInTheDocument()
  })

  it('shows Installed badge when agent already installed', async () => {
    listAgentCatalog.mockResolvedValue([nativeAgent])
    listInstalledAgentPlugins.mockResolvedValue([installedAgent])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getAllByText('Installed').length).toBeGreaterThan(0)
    })
  })

  it('installs native agent as a flat AgentDefinition (G1 P1-9)', async () => {
    listAgentCatalog.mockResolvedValue([nativeAgent])
    listInstalledAgentPlugins.mockResolvedValue([])
    installNativeAgent.mockResolvedValue({
      id: 'native:agent-code-reviewer',
      name: 'code-reviewer',
      install_path: '/home/user/.shannon/agents/code-reviewer.toml',
    })
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('code-reviewer')).toBeInTheDocument()
    })
    fireEvent.click(screen.getByText('Install'))
    await waitFor(() => {
      expect(installNativeAgent).toHaveBeenCalled()
    })
    // New structured signature: (name, description, systemPrompt, model, tools)
    // — no more ad-hoc markdown body.
    expect(installNativeAgent.mock.calls[0][0]).toBe('code-reviewer')
    expect(installNativeAgent.mock.calls[0][1]).toBe(
      'Reviews code for bugs, security issues, and best practices.',
    )
    expect(installNativeAgent.mock.calls[0][2]).toBe('You are a code reviewer.')
    expect(installNativeAgent.mock.calls[0][3]).toBe('claude-sonnet-4-6')
    expect(installNativeAgent.mock.calls[0][4]).toEqual(['read', 'grep', 'glob'])
  })

  it('falls back to the description when a catalog entry lacks system_prompt', async () => {
    const bodyless = {
      ...nativeAgent,
      metadata: { model: 'claude-sonnet-4-6', tools: ['read'] },
    }
    listAgentCatalog.mockResolvedValue([bodyless])
    listInstalledAgentPlugins.mockResolvedValue([])
    installNativeAgent.mockResolvedValue({
      id: 'native:agent-code-reviewer',
      name: 'code-reviewer',
      install_path: '/home/user/.shannon/agents/code-reviewer.toml',
    })
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('code-reviewer')).toBeInTheDocument()
    })
    fireEvent.click(screen.getByText('Install'))
    await waitFor(() => {
      expect(installNativeAgent).toHaveBeenCalled()
    })
    expect(installNativeAgent.mock.calls[0][2]).toBe(bodyless.description)
  })

  it('installs repo agent via installAgentFromRepo', async () => {
    listAgentCatalog.mockResolvedValue([repoAgent])
    listInstalledAgentPlugins.mockResolvedValue([])
    installAgentFromRepo.mockResolvedValue({
      id: 'gh:VoltAgent/awesome-claude-code-agents/main/doc-writer',
      name: 'doc-writer',
      install_path: '/home/user/.shannon/agents/doc-writer',
    })
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('doc-writer')).toBeInTheDocument()
    })
    fireEvent.click(screen.getByText('Install'))
    await waitFor(() => {
      expect(installAgentFromRepo).toHaveBeenCalledWith(
        'doc-writer',
        'VoltAgent/awesome-claude-code-agents',
        'main',
      )
    })
  })

  it('renders installed section with agent name and Remove button', async () => {
    listAgentCatalog.mockResolvedValue([])
    listInstalledAgentPlugins.mockResolvedValue([installedAgent])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText(/Installed · 1/)).toBeInTheDocument()
    })
    expect(screen.getByText('code-reviewer')).toBeInTheDocument()
    expect(screen.getByText('Remove')).toBeInTheDocument()
  })

  it('uninstalls agent on Remove click', async () => {
    listAgentCatalog.mockResolvedValue([])
    listInstalledAgentPlugins.mockResolvedValue([installedAgent])
    uninstallAgentPlugin.mockResolvedValue(undefined)
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('Remove')).toBeInTheDocument()
    })
    fireEvent.click(screen.getByText('Remove'))
    const dialog = await screen.findByRole('alertdialog', { name: /Remove agent\?/i })
    fireEvent.click(within(dialog).getByRole('button', { name: /^Remove$/ }))
    await waitFor(() => {
      expect(uninstallAgentPlugin).toHaveBeenCalledWith('code-reviewer')
    })
  })

  it('shows empty state when catalog is empty', async () => {
    listAgentCatalog.mockResolvedValue([])
    listInstalledAgentPlugins.mockResolvedValue([])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('No agents found.')).toBeInTheDocument()
    })
  })
})
