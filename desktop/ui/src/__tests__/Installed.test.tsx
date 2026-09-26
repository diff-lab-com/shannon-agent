import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor, fireEvent } from '@testing-library/react'
import { MemoryRouter, Routes, Route, Outlet } from 'react-router-dom'
import * as api from '@/lib/tauri-api'
import Installed from '@/components/extensions/Installed'
import type { ExtensionStats } from '@/types'

vi.mock('@/lib/tauri-api', () => ({
  listInstalledAddons: vi.fn(),
  getExtensionStats: vi.fn(),
}))

/** Stats payload with no data anywhere — the "no subtext" default. */
const emptyStats: ExtensionStats = { days: 30, skills: [], mcpServers: [], other: [] }

/// Wraps Installed in the same Outlet context shape the Extensions page
/// provides (`<Outlet context={{ search }} />`). Without this, `useOutletContext`
/// returns null and the component throws on mount.
function WithSearchOutlet({ search = '' }: { search?: string }) {
  return (
    <>
      <Outlet context={{ search }} />
    </>
  )
}

const sampleRows = [
  { id: 'mcp:notion', kind: 'mcp' as const, name: 'notion', install_path: '~/.shannon/settings.json#mcpServers.notion (user)', installed_at: '2026-06-10T00:00:00Z', enabled: true },
  { id: 'mcp:disabled', kind: 'mcp' as const, name: 'disabled', install_path: '~/.shannon/settings.json#mcpServers.disabled (user)', installed_at: '2026-06-10T00:00:00Z', enabled: false },
  { id: 'skill:deploy', kind: 'skill' as const, name: 'deploy', install_path: '/home/u/.shannon/skills/deploy.md', enabled: true },
  { id: 'agent:reviewer', kind: 'agent' as const, name: 'reviewer', install_path: '/home/u/.shannon/agents/reviewer.md', enabled: true },
]

function renderInstalled() {
  return render(
    <MemoryRouter initialEntries={['/extensions/installed']}>
      <Routes>
        <Route path="*" element={<WithSearchOutlet />}>
          <Route path="*" element={<Installed />} />
        </Route>
      </Routes>
    </MemoryRouter>
  )
}

describe('Installed extensions tab', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    // X7 stats default to "no data" so the row layout stays untouched
    // unless a test opts in.
    vi.mocked(api.getExtensionStats).mockResolvedValue(emptyStats)
  })

  it('shows loading state initially', async () => {
    vi.mocked(api.listInstalledAddons).mockReturnValue(new Promise(() => {}))
    renderInstalled()
    expect(screen.getByText('Scanning local configs…')).toBeInTheDocument()
  })

  it('shows error state when API fails', async () => {
    vi.mocked(api.listInstalledAddons).mockRejectedValueOnce(new Error('disk corruption'))
    renderInstalled()
    await waitFor(() => {
      expect(screen.getByText('Failed to load installed addons')).toBeInTheDocument()
      expect(screen.getByText(/disk corruption/)).toBeInTheDocument()
    })
  })

  it('shows empty state when no addons installed', async () => {
    vi.mocked(api.listInstalledAddons).mockResolvedValueOnce([])
    renderInstalled()
    await waitFor(() => {
      expect(screen.getByText('Nothing installed yet')).toBeInTheDocument()
    })
  })

  it('shows Browse catalog CTA in empty state', async () => {
    vi.mocked(api.listInstalledAddons).mockResolvedValueOnce([])
    renderInstalled()
    await waitFor(() => {
      expect(screen.getByText('Browse catalog')).toBeInTheDocument()
    })
  })

  it('groups addons by kind with correct category labels', async () => {
    vi.mocked(api.listInstalledAddons).mockResolvedValueOnce(sampleRows)
    renderInstalled()
    await waitFor(() => {
      expect(screen.getByText('MCP Servers · 2')).toBeInTheDocument()
      expect(screen.getByText('Skills · 1')).toBeInTheDocument()
      expect(screen.getByText('Agents · 1')).toBeInTheDocument()
    })
  })

  it('renders addon names', async () => {
    vi.mocked(api.listInstalledAddons).mockResolvedValueOnce(sampleRows)
    renderInstalled()
    await waitFor(() => {
      expect(screen.getByText('notion')).toBeInTheDocument()
      expect(screen.getByText('deploy')).toBeInTheDocument()
      expect(screen.getByText('reviewer')).toBeInTheDocument()
    })
  })

  it('shows Disabled badge for disabled addons', async () => {
    vi.mocked(api.listInstalledAddons).mockResolvedValueOnce(sampleRows)
    renderInstalled()
    await waitFor(() => {
      expect(screen.getByText('Disabled')).toBeInTheDocument()
    })
  })

  it('shows install path in mono font', async () => {
    vi.mocked(api.listInstalledAddons).mockResolvedValueOnce([sampleRows[0]])
    renderInstalled()
    await waitFor(() => {
      expect(screen.getByText(/notion \(user\)/)).toBeInTheDocument()
    })
  })

  it('shows header with entry count', async () => {
    vi.mocked(api.listInstalledAddons).mockResolvedValueOnce(sampleRows)
    renderInstalled()
    // The h1 was retired; the entry count is now the page's distinctive marker.
    await waitFor(() => {
      expect(screen.getByText(/4 entries across 3 categories/)).toBeInTheDocument()
    })
  })

  it('shows singular "entry" for 1 row', async () => {
    vi.mocked(api.listInstalledAddons).mockResolvedValueOnce([sampleRows[0]])
    renderInstalled()
    await waitFor(() => {
      expect(screen.getByText(/1 entry across 1 category/)).toBeInTheDocument()
    })
  })

  // X4 锚点分区: jump chips per populated kind, clicking one scrolls the
  // matching section into view.
  describe('X4 anchor jump chips', () => {
    it('renders one chip per populated kind with its count', async () => {
      vi.mocked(api.listInstalledAddons).mockResolvedValueOnce(sampleRows)
      renderInstalled()
      await waitFor(() => {
        expect(screen.getByTestId('installed-jump-nav')).toBeInTheDocument()
      })
      expect(screen.getByTestId('installed-jump-mcp')).toHaveTextContent('MCP Servers')
      expect(screen.getByTestId('installed-jump-mcp')).toHaveTextContent('2')
      expect(screen.getByTestId('installed-jump-skill')).toHaveTextContent('Skills')
      expect(screen.getByTestId('installed-jump-agent')).toHaveTextContent('Agents')
      // Empty kinds get no chip.
      expect(screen.queryByTestId('installed-jump-plugin')).not.toBeInTheDocument()
      expect(screen.queryByTestId('installed-jump-data_source')).not.toBeInTheDocument()
    })

    it('scrolls the matching section into view on click', async () => {
      vi.mocked(api.listInstalledAddons).mockResolvedValueOnce(sampleRows)
      let scrolledTo: string | null = null
      const spy = vi
        .spyOn(Element.prototype, 'scrollIntoView')
        .mockImplementation(function (this: Element) {
          scrolledTo = (this as HTMLElement).id
        })
      renderInstalled()
      await waitFor(() => {
        expect(screen.getByTestId('installed-jump-skill')).toBeInTheDocument()
      })
      fireEvent.click(screen.getByTestId('installed-jump-skill'))
      expect(scrolledTo).toBe('installed-section-skill')
      expect(document.getElementById('installed-section-skill')).not.toBeNull()
      spy.mockRestore()
    })

    it('hides the jump nav when only one kind is populated', async () => {
      vi.mocked(api.listInstalledAddons).mockResolvedValueOnce([sampleRows[0]])
      renderInstalled()
      await waitFor(() => {
        expect(screen.getByText('notion')).toBeInTheDocument()
      })
      expect(screen.queryByTestId('installed-jump-nav')).not.toBeInTheDocument()
    })
  })

  // X7 — per-extension usage subtext (「30 天调用 N 次 · ~X tokens」).
  describe('X7 usage stats subtext', () => {
    const statsWithData: ExtensionStats = {
      days: 30,
      skills: [{ name: 'deploy', calls: 12, totalTokens: 3500 }],
      mcpServers: [
        { server: 'notion', calls: 7, totalTokens: 1200, tools: [{ name: 'search', calls: 7, totalTokens: 1200 }] },
        { server: 'disabled', calls: 0, totalTokens: 0, tools: [] },
      ],
      other: [],
    }

    it('fetches stats once per mount with the 30-day default window', async () => {
      vi.mocked(api.listInstalledAddons).mockResolvedValueOnce(sampleRows)
      vi.mocked(api.getExtensionStats).mockResolvedValueOnce(statsWithData)
      renderInstalled()
      await waitFor(() => {
        expect(api.getExtensionStats).toHaveBeenCalledTimes(1)
      })
      expect(api.getExtensionStats).toHaveBeenCalledWith(30)
    })

    it('renders the subtext for matching skill and mcp rows', async () => {
      vi.mocked(api.listInstalledAddons).mockResolvedValueOnce(sampleRows)
      vi.mocked(api.getExtensionStats).mockResolvedValueOnce(statsWithData)
      renderInstalled()
      await waitFor(() => {
        expect(screen.getAllByTestId('installed-row-stats')).toHaveLength(2)
      })
      // Skill row matches by name; MCP row matches by server name.
      expect(screen.getByText('12 calls in 30 days · ~3500 tokens')).toBeInTheDocument()
      expect(screen.getByText('7 calls in 30 days · ~1200 tokens')).toBeInTheDocument()
    })

    it('renders the tokens segment only when tokens are present', async () => {
      vi.mocked(api.listInstalledAddons).mockResolvedValueOnce(sampleRows)
      vi.mocked(api.getExtensionStats).mockResolvedValueOnce({
        ...statsWithData,
        skills: [{ name: 'deploy', calls: 3, totalTokens: 0 }],
      })
      renderInstalled()
      await waitFor(() => {
        expect(screen.getByText('3 calls in 30 days')).toBeInTheDocument()
      })
      expect(screen.getByText('3 calls in 30 days').textContent).not.toContain('tokens')
    })

    it('renders no subtext when stats carry no data', async () => {
      vi.mocked(api.listInstalledAddons).mockResolvedValueOnce(sampleRows)
      vi.mocked(api.getExtensionStats).mockResolvedValueOnce(emptyStats)
      renderInstalled()
      await waitFor(() => {
        expect(screen.getByText('deploy')).toBeInTheDocument()
      })
      expect(screen.queryByTestId('installed-row-stats')).not.toBeInTheDocument()
    })

    it('renders no subtext when the stats fetch fails', async () => {
      vi.mocked(api.listInstalledAddons).mockResolvedValueOnce(sampleRows)
      vi.mocked(api.getExtensionStats).mockRejectedValueOnce(new Error('stats down'))
      renderInstalled()
      await waitFor(() => {
        expect(screen.getByText('deploy')).toBeInTheDocument()
      })
      expect(screen.queryByTestId('installed-row-stats')).not.toBeInTheDocument()
    })
  })
})
