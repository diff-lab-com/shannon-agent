import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor, fireEvent } from '@testing-library/react'
import { MemoryRouter, Routes, Route, Outlet } from 'react-router-dom'
import McpServers from '@/components/extensions/McpServers'

function Shell() {
  return <Outlet context={{ search: '' }} />
}

const listMcpRegistryServers = vi.hoisted(() => vi.fn())
const listMcpServers = vi.hoisted(() => vi.fn())
const installMcpStdio = vi.hoisted(() => vi.fn())
const installMcpMcpb = vi.hoisted(() => vi.fn())
const uninstallMcpServer = vi.hoisted(() => vi.fn())

vi.mock('@/lib/tauri-api', () => ({
  default: {},
  listMcpRegistryServers: (...a: unknown[]) => listMcpRegistryServers(...a),
  listMcpServers: (...a: unknown[]) => listMcpServers(...a),
  installMcpStdio: (...a: unknown[]) => installMcpStdio(...a),
  installMcpMcpb: (...a: unknown[]) => installMcpMcpb(...a),
  uninstallMcpServer: (...a: unknown[]) => uninstallMcpServer(...a),
}))

function renderWithRouter() {
  return render(
    <MemoryRouter initialEntries={['/extensions/mcp-servers']}>
      <Routes>
        <Route path="/*" element={<Shell />}>
          <Route path="*" element={<McpServers />} />
        </Route>
      </Routes>
    </MemoryRouter>
  )
}

const sampleServer = {
  id: 'filesystem',
  name: 'filesystem',
  description: 'Filesystem MCP server',
  repository: 'https://github.com/example/fs',
  version: '1.0.0',
  homepage_url: null,
  license: 'MIT',
  stars: 123,
  last_updated: '2026-01-01',
  verified: true,
}

const sampleInstalled = {
  name: 'filesystem',
  command: 'npx',
  args: ['-y', '@modelcontextprotocol/server-filesystem'],
  env: {},
  status: 'running',
} as const

beforeEach(() => {
  listMcpRegistryServers.mockReset()
  listMcpServers.mockReset()
  installMcpStdio.mockReset()
  installMcpMcpb.mockReset()
  uninstallMcpServer.mockReset()
})

describe('McpServers (P2 wire-up)', () => {
  it('renders three section headers', async () => {
    listMcpRegistryServers.mockResolvedValue([])
    listMcpServers.mockResolvedValue([])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText(/Registry · 0/)).toBeInTheDocument()
    })
    expect(screen.getByText('Upload .mcpb bundle')).toBeInTheDocument()
    expect(screen.getByText('Add stdio server manually')).toBeInTheDocument()
  })

  it('shows loading state for registry', () => {
    listMcpRegistryServers.mockReturnValue(new Promise(() => {}))
    listMcpServers.mockResolvedValue([])
    renderWithRouter()
    expect(screen.getByText('Fetching registry…')).toBeInTheDocument()
  })

  it('shows error state when registry fetch fails', async () => {
    listMcpRegistryServers.mockRejectedValue(new Error('Network down'))
    listMcpServers.mockResolvedValue([])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText(/Registry unavailable/)).toBeInTheDocument()
    })
    expect(screen.getByText(/Network down/)).toBeInTheDocument()
  })

  it('renders registry rows with Install button', async () => {
    listMcpRegistryServers.mockResolvedValue([sampleServer])
    listMcpServers.mockResolvedValue([])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('filesystem')).toBeInTheDocument()
    })
    expect(screen.getByText('Repo')).toBeInTheDocument()
    // Registry install button + stdio form Install button — registry has "Install" and "Repo"
    expect(screen.getByText('Repo')).toBeInTheDocument()
    expect(screen.getByText('123')).toBeInTheDocument()
    expect(screen.getByText('Verified')).toBeInTheDocument()
  })

  it('shows Installed badge when server already installed', async () => {
    listMcpRegistryServers.mockResolvedValue([sampleServer])
    listMcpServers.mockResolvedValue([sampleInstalled])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getAllByText('Installed').length).toBeGreaterThanOrEqual(2)
    })
    // Section header + registry badge
    const installedMatches = screen.getAllByText('Installed')
    expect(installedMatches.length).toBe(2)
  })

  it('renders installed section with server name', async () => {
    listMcpRegistryServers.mockResolvedValue([])
    listMcpServers.mockResolvedValue([sampleInstalled])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText(/Installed · 1/)).toBeInTheDocument()
    })
    expect(screen.getByText('filesystem')).toBeInTheDocument()
    expect(screen.getByText('Remove')).toBeInTheDocument()
  })

  it('requires name + command for stdio install', async () => {
    listMcpRegistryServers.mockResolvedValue([])
    listMcpServers.mockResolvedValue([])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('Install').closest('button')).not.toBeDisabled()
    })
    fireEvent.click(screen.getAllByText('Install')[screen.getAllByText('Install').length - 1])
    await waitFor(() => {
      expect(screen.getByText(/Server name and command are required/)).toBeInTheDocument()
    })
    expect(installMcpStdio).not.toHaveBeenCalled()
  })

  it('uninstalls server on Remove click', async () => {
    listMcpRegistryServers.mockResolvedValue([])
    listMcpServers.mockResolvedValue([sampleInstalled])
    uninstallMcpServer.mockResolvedValue(undefined)
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('Remove')).toBeInTheDocument()
    })
    fireEvent.click(screen.getByText('Remove'))
    await waitFor(() => {
      expect(uninstallMcpServer).toHaveBeenCalledWith('filesystem')
    })
  })

  it('validates stdio form submission with full spec', async () => {
    listMcpRegistryServers.mockResolvedValue([])
    listMcpServers.mockResolvedValue([])
    installMcpStdio.mockResolvedValue({
      id: 'stdio:myserver',
      name: 'myserver',
      install_path: null,
    })
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByPlaceholderText('filesystem')).toBeInTheDocument()
    })

    fireEvent.change(screen.getByPlaceholderText('filesystem'), { target: { value: 'myserver' } })
    fireEvent.change(screen.getByPlaceholderText('npx'), { target: { value: 'npx' } })
    fireEvent.change(screen.getByPlaceholderText(/-y @modelcontextprotocol/), {
      target: { value: '-y @modelcontextprotocol/server-filesystem /tmp' },
    })

    const installButton = screen.getAllByText('Install').pop()!.closest('button')!
    fireEvent.click(installButton)

    await waitFor(() => {
      expect(installMcpStdio).toHaveBeenCalledWith({
        server_name: 'myserver',
        command: 'npx',
        args: ['-y', '@modelcontextprotocol/server-filesystem', '/tmp'],
        env: [],
      })
    })
  })
})
