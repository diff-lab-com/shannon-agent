import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor, fireEvent, within } from '@testing-library/react'
import { MemoryRouter, Routes, Route, Outlet } from 'react-router-dom'
import Skills from '@/components/extensions/Skills'

function Shell() {
  return <Outlet context={{ search: '' }} />
}

const listSkillCatalog = vi.hoisted(() => vi.fn())
const listInstalledSkillPlugins = vi.hoisted(() => vi.fn())
const installNativeSkill = vi.hoisted(() => vi.fn())
const installSkillFromRepo = vi.hoisted(() => vi.fn())
const uninstallSkillPlugin = vi.hoisted(() => vi.fn())

vi.mock('@/lib/tauri-api', () => ({
  default: {},
  listSkillCatalog: (...a: unknown[]) => listSkillCatalog(...a),
  listInstalledSkillPlugins: (...a: unknown[]) => listInstalledSkillPlugins(...a),
  installNativeSkill: (...a: unknown[]) => installNativeSkill(...a),
  installSkillFromRepo: (...a: unknown[]) => installSkillFromRepo(...a),
  uninstallSkillPlugin: (...a: unknown[]) => uninstallSkillPlugin(...a),
}))

function renderWithRouter() {
  return render(
    <MemoryRouter initialEntries={['/extensions/skills']}>
      <Routes>
        <Route path="/*" element={<Shell />}>
          <Route path="*" element={<Skills />} />
        </Route>
      </Routes>
    </MemoryRouter>
  )
}

const nativeSkill = {
  id: 'native:pdf',
  kind: 'skill' as const,
  name: 'PDF Toolkit',
  description: 'Read, search, and extract content from PDF documents.',
  author: 'Shannon',
  version: '0.1.0',
  homepage_url: null,
  license: 'Apache-2.0',
  stars: null,
  last_updated: null,
  source: { type: 'native' as const },
  trust: 'verified' as const,
  metadata: {},
  tags: ['pdf', 'native'],
}

const repoSkill = {
  id: 'gh:anthropics/skills/main/brainstorming',
  kind: 'skill' as const,
  name: 'brainstorming',
  description: 'Socratic dialogue for requirements discovery.',
  author: 'anthropics/skills',
  version: '1.0.0',
  homepage_url: 'https://github.com/anthropics/skills',
  license: null,
  stars: null,
  last_updated: null,
  source: { type: 'git_hub_repo' as const, repo: 'anthropics/skills', ref_: 'main' },
  trust: 'verified' as const,
  metadata: {},
  tags: ['discovery'],
}

const installedSkill = {
  name: 'PDF Toolkit',
  path: '/home/user/.shannon/skills/PDF Toolkit',
  installed_at: '2026-06-15T00:00:00Z',
}

beforeEach(() => {
  listSkillCatalog.mockReset()
  listInstalledSkillPlugins.mockReset()
  installNativeSkill.mockReset()
  installSkillFromRepo.mockReset()
  uninstallSkillPlugin.mockReset()
})

describe('Skills (P3 federated catalog)', () => {
  it('renders catalog and installed headers', async () => {
    listSkillCatalog.mockResolvedValue([])
    listInstalledSkillPlugins.mockResolvedValue([])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText(/Catalog · 0/)).toBeInTheDocument()
    })
    expect(screen.getByText(/Installed · 0/)).toBeInTheDocument()
  })

  it('shows loading state for catalog', () => {
    listSkillCatalog.mockReturnValue(new Promise(() => {}))
    listInstalledSkillPlugins.mockResolvedValue([])
    renderWithRouter()
    expect(screen.getByText('Fetching skill catalog…')).toBeInTheDocument()
  })

  it('shows error state when catalog fetch fails', async () => {
    listSkillCatalog.mockRejectedValue(new Error('Network down'))
    listInstalledSkillPlugins.mockResolvedValue([])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText(/Failed to load catalog:/)).toBeInTheDocument()
    })
    expect(screen.getByText(/Network down/)).toBeInTheDocument()
  })

  it('renders catalog entries with trust badges', async () => {
    listSkillCatalog.mockResolvedValue([nativeSkill, repoSkill])
    listInstalledSkillPlugins.mockResolvedValue([])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('PDF Toolkit')).toBeInTheDocument()
    })
    expect(screen.getByText('brainstorming')).toBeInTheDocument()
    expect(screen.getAllByText('Verified').length).toBeGreaterThan(0)
    expect(screen.getAllByText('Install').length).toBeGreaterThanOrEqual(2)
  })

  it('shows Installed badge when skill already installed', async () => {
    listSkillCatalog.mockResolvedValue([nativeSkill])
    listInstalledSkillPlugins.mockResolvedValue([installedSkill])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('Installed')).toBeInTheDocument()
    })
    // Card button shows Installed (disabled); section also shows it.
    const installedMatches = screen.getAllByText('Installed')
    expect(installedMatches.length).toBeGreaterThanOrEqual(1)
  })

  it('installs native skill on click', async () => {
    listSkillCatalog.mockResolvedValue([nativeSkill])
    listInstalledSkillPlugins.mockResolvedValue([])
    installNativeSkill.mockResolvedValue({
      id: 'native:pdf',
      name: 'PDF Toolkit',
      install_path: '/home/user/.shannon/skills/PDF Toolkit/SKILL.md',
    })
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('PDF Toolkit')).toBeInTheDocument()
    })
    fireEvent.click(screen.getByText('Install'))
    await waitFor(() => {
      expect(installNativeSkill).toHaveBeenCalled()
    })
    expect(installNativeSkill.mock.calls[0][0]).toBe('PDF Toolkit')
    // Body should be a SKILL.md stub with frontmatter and description.
    const body = installNativeSkill.mock.calls[0][1] as string
    expect(body).toContain('name: PDF Toolkit')
    expect(body).toContain('Read, search, and extract content from PDF')
  })

  it('installs repo skill via installSkillFromRepo', async () => {
    listSkillCatalog.mockResolvedValue([repoSkill])
    listInstalledSkillPlugins.mockResolvedValue([])
    installSkillFromRepo.mockResolvedValue({
      id: 'gh:anthropics/skills/main/brainstorming',
      name: 'brainstorming',
      install_path: '/home/user/.shannon/skills/brainstorming',
    })
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('brainstorming')).toBeInTheDocument()
    })
    fireEvent.click(screen.getByText('Install'))
    await waitFor(() => {
      expect(installSkillFromRepo).toHaveBeenCalledWith(
        'brainstorming',
        'anthropics/skills',
        'main',
      )
    })
  })

  it('renders installed section with skill name and Remove button', async () => {
    listSkillCatalog.mockResolvedValue([])
    listInstalledSkillPlugins.mockResolvedValue([installedSkill])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText(/Installed · 1/)).toBeInTheDocument()
    })
    expect(screen.getByText('PDF Toolkit')).toBeInTheDocument()
    expect(screen.getByText('Remove')).toBeInTheDocument()
  })

  it('uninstalls skill on Remove click', async () => {
    listSkillCatalog.mockResolvedValue([])
    listInstalledSkillPlugins.mockResolvedValue([installedSkill])
    uninstallSkillPlugin.mockResolvedValue(undefined)
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('Remove')).toBeInTheDocument()
    })
    fireEvent.click(screen.getByText('Remove'))
    const dialog = await screen.findByRole('alertdialog', { name: /Remove skill\?/i })
    fireEvent.click(within(dialog).getByRole('button', { name: /^Remove$/ }))
    await waitFor(() => {
      expect(uninstallSkillPlugin).toHaveBeenCalledWith('PDF Toolkit')
    })
  })

  it('shows empty state when catalog is empty', async () => {
    listSkillCatalog.mockResolvedValue([])
    listInstalledSkillPlugins.mockResolvedValue([])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('No skills found.')).toBeInTheDocument()
    })
  })

  it('opens detail drawer when card title is clicked', async () => {
    listSkillCatalog.mockResolvedValue([repoSkill])
    listInstalledSkillPlugins.mockResolvedValue([])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('brainstorming')).toBeInTheDocument()
    })
    fireEvent.click(screen.getByText('brainstorming'))
    await waitFor(() => {
      expect(screen.getByRole('dialog', { name: /brainstorming/i })).toBeInTheDocument()
    })
    // Drawer shows the "Source" / "Homepage" labels — card doesn't.
    expect(screen.getByText('Source')).toBeInTheDocument()
    expect(screen.getByText('Homepage')).toBeInTheDocument()
    // Source line in the drawer renders repo + ref.
    expect(screen.getByText('anthropics/skills @ main')).toBeInTheDocument()
  })

  it('closes detail drawer on backdrop click', async () => {
    listSkillCatalog.mockResolvedValue([repoSkill])
    listInstalledSkillPlugins.mockResolvedValue([])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('brainstorming')).toBeInTheDocument()
    })
    fireEvent.click(screen.getByText('brainstorming'))
    await waitFor(() => {
      expect(screen.getByRole('dialog', { name: /brainstorming/i })).toBeInTheDocument()
    })
    fireEvent.click(screen.getByRole('dialog', { name: /brainstorming/i }))
    await waitFor(() => {
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
    })
  })
})
