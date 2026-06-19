import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter, Routes, Route, Outlet } from 'react-router-dom'
import * as api from '@/lib/tauri-api'
import Installed from '@/components/extensions/Installed'

vi.mock('@/lib/tauri-api', () => ({
  listInstalledAddons: vi.fn(),
}))

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
    await waitFor(() => {
      expect(screen.getByText('Installed Extensions')).toBeInTheDocument()
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
})
