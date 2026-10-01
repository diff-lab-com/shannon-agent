import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor, fireEvent, within } from '@testing-library/react'
import { MemoryRouter, Routes, Route, Outlet, useLocation } from 'react-router-dom'
import McpServers from '@/components/extensions/McpServers'

function Shell() {
  return <Outlet context={{ search: '' }} />
}

const listMcpRegistryServers = vi.hoisted(() => vi.fn())
const listMcpServers = vi.hoisted(() => vi.fn())
const installMcpStdio = vi.hoisted(() => vi.fn())
const installMcpMcpb = vi.hoisted(() => vi.fn())
const uninstallMcpServer = vi.hoisted(() => vi.fn())
const restartMcpServer = vi.hoisted(() => vi.fn())
const setMcpServerEnabled = vi.hoisted(() => vi.fn())

vi.mock('@/lib/tauri-api', () => ({
  default: {},
  listMcpRegistryServers: (...a: unknown[]) => listMcpRegistryServers(...a),
  listMcpServers: (...a: unknown[]) => listMcpServers(...a),
  installMcpStdio: (...a: unknown[]) => installMcpStdio(...a),
  installMcpMcpb: (...a: unknown[]) => installMcpMcpb(...a),
  uninstallMcpServer: (...a: unknown[]) => uninstallMcpServer(...a),
  restartMcpServer: (...a: unknown[]) => restartMcpServer(...a),
  setMcpServerEnabled: (...a: unknown[]) => setMcpServerEnabled(...a),
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
  enabled: true,
  connected: true,
  tool_count: 5,
  tools: [],
  last_connected: null,
  has_auth_headers: false,
}

// W1-A honest state: an OAuth-product url-only row (headers on the store
// blob) — backend verdict `has_auth_headers: true`.
const sampleRemote = {
  name: 'notion',
  command: '',
  enabled: true,
  connected: false,
  tool_count: 0,
  tools: [],
  last_connected: null,
  url: 'https://mcp.notion.com/mcp',
  has_auth_headers: true,
}

// W2-A (R4/A1): a header-less url-only row — pure remote, wired into the
// pool, rendering exactly like stdio.
const samplePureRemote = {
  name: 'deepwiki',
  command: '',
  enabled: true,
  connected: true,
  tool_count: 3,
  tools: [],
  last_connected: 1735689600000,
  url: 'https://mcp.deepwiki.com/mcp',
  has_auth_headers: false,
}

const sampleFailed = {
  name: 'broken',
  command: 'npx',
  enabled: true,
  connected: false,
  tool_count: 0,
  tools: [],
  last_connected: null,
  has_auth_headers: false,
  last_error: 'spawn /nonexistent ENOENT',
}

const sampleFailedRemote = {
  name: 'flaky-remote',
  command: '',
  enabled: true,
  connected: false,
  tool_count: 0,
  tools: [],
  last_connected: null,
  url: 'http://127.0.0.1:1/mcp',
  has_auth_headers: false,
  last_error: "Remote MCP server 'flaky-remote' returned HTTP 500",
}

const sampleDisabled = {
  name: 'paused',
  command: 'npx',
  enabled: false,
  connected: false,
  tool_count: 0,
  tools: [],
  last_connected: null,
  has_auth_headers: false,
}

beforeEach(() => {
  listMcpRegistryServers.mockReset()
  listMcpServers.mockReset()
  installMcpStdio.mockReset()
  installMcpMcpb.mockReset()
  uninstallMcpServer.mockReset()
  restartMcpServer.mockReset()
  setMcpServerEnabled.mockReset()
  // Default: registry returns empty so any test opening the modal won't crash.
  listMcpRegistryServers.mockResolvedValue([])
})

describe('McpServers (Cursor-style UX)', () => {
  it('renders page header and empty state when nothing installed', async () => {
    listMcpServers.mockResolvedValue([])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('MCP Servers')).toBeInTheDocument()
    })
    // Installed section header with 0 count
    expect(screen.getByText(/Installed · 0/)).toBeInTheDocument()
    // Empty state body
    expect(
      screen.getByText(/Click 'Add Server' to install your first MCP server\./),
    ).toBeInTheDocument()
  })

  it('renders Add Server CTA button', async () => {
    listMcpServers.mockResolvedValue([])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /Add Server/ })).toBeInTheDocument()
    })
  })

  it('renders installed servers with name, status, and Remove button', async () => {
    listMcpServers.mockResolvedValue([sampleInstalled])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText(/Installed · 1/)).toBeInTheDocument()
    })
    expect(screen.getByText('filesystem')).toBeInTheDocument()
    expect(screen.getByText('Remove')).toBeInTheDocument()
    // Command preview (mono)
    expect(screen.getByText('npx')).toBeInTheDocument()
  })

  it('shows empty state title when no servers installed', async () => {
    listMcpServers.mockResolvedValue([])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('No MCP servers installed')).toBeInTheDocument()
    })
  })

  it('opens the modal with three tabs when Add Server is clicked', async () => {
    listMcpServers.mockResolvedValue([])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /Add Server/ })).toBeInTheDocument()
    })
    fireEvent.click(screen.getByRole('button', { name: /Add Server/ }))
    await waitFor(() => {
      expect(screen.getByText('Add MCP Server')).toBeInTheDocument()
    })
    expect(screen.getByRole('tab', { name: 'Search' })).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: 'Paste JSON' })).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: 'Manual' })).toBeInTheDocument()
  })

  it('search tab lists registry rows after loading', async () => {
    listMcpServers.mockResolvedValue([])
    listMcpRegistryServers.mockResolvedValue([sampleServer])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /Add Server/ })).toBeInTheDocument()
    })
    fireEvent.click(screen.getByRole('button', { name: /Add Server/ }))
    await waitFor(() => {
      expect(screen.getByText('filesystem')).toBeInTheDocument()
    })
    expect(screen.getByText('Verified')).toBeInTheDocument()
  })

  it('search tab shows empty message when query matches nothing', async () => {
    listMcpServers.mockResolvedValue([])
    listMcpRegistryServers.mockResolvedValue([sampleServer])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /Add Server/ })).toBeInTheDocument()
    })
    fireEvent.click(screen.getByRole('button', { name: /Add Server/ }))
    await waitFor(() => {
      expect(screen.getByPlaceholderText('Search registry…')).toBeInTheDocument()
    })
    fireEvent.change(screen.getByPlaceholderText('Search registry…'), {
      target: { value: 'zzzznotfound' },
    })
    await waitFor(() => {
      expect(screen.getByText('No servers match your query.')).toBeInTheDocument()
    })
  })

  it('manual tab requires name + command and shows error via toast', async () => {
    listMcpServers.mockResolvedValue([])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /Add Server/ })).toBeInTheDocument()
    })
    fireEvent.click(screen.getByRole('button', { name: /Add Server/ }))
    fireEvent.click(screen.getByRole('tab', { name: 'Manual' }))
    await waitFor(() => {
      expect(screen.getByPlaceholderText('filesystem')).toBeInTheDocument()
    })
    // Click install without filling required fields
    const manualInstallButtons = screen.getAllByRole('button', { name: /Install/ })
    fireEvent.click(manualInstallButtons[manualInstallButtons.length - 1])
    await waitFor(() => {
      expect(installMcpStdio).not.toHaveBeenCalled()
    })
  })

  it('manual tab submits full stdio spec', async () => {
    listMcpServers.mockResolvedValue([])
    installMcpStdio.mockResolvedValue({
      id: 'stdio:myserver',
      name: 'myserver',
      install_path: null,
    })
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /Add Server/ })).toBeInTheDocument()
    })
    fireEvent.click(screen.getByRole('button', { name: /Add Server/ }))
    fireEvent.click(screen.getByRole('tab', { name: 'Manual' }))
    await waitFor(() => {
      expect(screen.getByPlaceholderText('filesystem')).toBeInTheDocument()
    })

    fireEvent.change(screen.getByPlaceholderText('filesystem'), { target: { value: 'myserver' } })
    fireEvent.change(screen.getByPlaceholderText('npx'), { target: { value: 'npx' } })
    fireEvent.change(screen.getByPlaceholderText(/-y @modelcontextprotocol/), {
      target: { value: '-y @modelcontextprotocol/server-filesystem /tmp' },
    })

    const installButton = screen.getAllByRole('button', { name: /Install/ }).pop()!
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

  it('paste tab parses Cursor-format JSON', async () => {
    listMcpServers.mockResolvedValue([])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /Add Server/ })).toBeInTheDocument()
    })
    fireEvent.click(screen.getByRole('button', { name: /Add Server/ }))
    fireEvent.click(screen.getByRole('tab', { name: 'Paste JSON' }))
    await waitFor(() => {
      expect(screen.getByPlaceholderText('Paste your MCP server JSON here')).toBeInTheDocument()
    })

    const json = JSON.stringify({
      mcpServers: {
        'my-server': {
          command: 'npx',
          args: ['-y', 'foo'],
          env: { KEY: 'val' },
        },
      },
    })
    fireEvent.change(screen.getByPlaceholderText('Paste your MCP server JSON here'), {
      target: { value: json },
    })
    // Parsed server appears in the preview list and the Install button shows count
    await waitFor(() => {
      expect(screen.getByText('Install 1 server(s)')).toBeInTheDocument()
    })
    // The parsed server name renders in the preview list
    expect(screen.getAllByText('my-server').length).toBeGreaterThan(0)
  })

  it('paste tab shows parse error for malformed JSON', async () => {
    listMcpServers.mockResolvedValue([])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /Add Server/ })).toBeInTheDocument()
    })
    fireEvent.click(screen.getByRole('button', { name: /Add Server/ }))
    fireEvent.click(screen.getByRole('tab', { name: 'Paste JSON' }))
    await waitFor(() => {
      expect(screen.getByPlaceholderText('Paste your MCP server JSON here')).toBeInTheDocument()
    })

    fireEvent.change(screen.getByPlaceholderText('Paste your MCP server JSON here'), {
      target: { value: '{not valid json' },
    })
    await waitFor(() => {
      expect(screen.getByText(/Could not parse JSON/)).toBeInTheDocument()
    })
  })

  it('uninstalls server on Remove click', async () => {
    listMcpServers.mockResolvedValue([sampleInstalled])
    uninstallMcpServer.mockResolvedValue(undefined)
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('Remove')).toBeInTheDocument()
    })
    fireEvent.click(screen.getByText('Remove'))
    const dialog = await screen.findByRole('alertdialog', { name: /Remove MCP server\?/i })
    fireEvent.click(within(dialog).getByRole('button', { name: /^Remove$/ }))
    await waitFor(() => {
      expect(uninstallMcpServer).toHaveBeenCalledWith('filesystem')
    })
  })

  // G1 P0-1.4 — the row's Restart button wires to the existing
  // `restart_mcp_server` backend and refreshes the list afterwards.
  it('restarts a server from its row', async () => {
    listMcpServers.mockResolvedValue([sampleInstalled])
    restartMcpServer.mockResolvedValue({ ...sampleInstalled, connected: true })
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Restart filesystem' })).toBeInTheDocument()
    })
    fireEvent.click(screen.getByRole('button', { name: 'Restart filesystem' }))
    await waitFor(() => {
      expect(restartMcpServer).toHaveBeenCalledWith('filesystem')
    })
    // The refresh re-reads the installed list.
    await waitFor(() => {
      expect(listMcpServers.mock.calls.length).toBeGreaterThanOrEqual(2)
    })
  })

  // X3 权限就近直达: the row's 工具权限 button deep links into the
  // permissions page with the server scope pre-selected (URL-encoded
  // `mcp:<name>`).
  it('deep links the tool-permissions button to the scoped permissions page', async () => {
    listMcpServers.mockResolvedValue([sampleInstalled])
    function LocationProbe() {
      const { search } = useLocation()
      return <div data-testid="probe" data-search={search} />
    }
    render(
      <MemoryRouter initialEntries={['/extensions/mcp-servers']}>
        <Routes>
          <Route path="/*" element={<Shell />}>
            <Route path="*" element={<McpServers />} />
          </Route>
          <Route path="/settings/permissions" element={<LocationProbe />} />
        </Routes>
      </MemoryRouter>,
    )
    await waitFor(() => {
      expect(
        screen.getByRole('button', { name: 'Tool permissions for filesystem' }),
      ).toBeInTheDocument()
    })
    fireEvent.click(screen.getByRole('button', { name: 'Tool permissions for filesystem' }))
    await waitFor(() => {
      expect(screen.getByTestId('probe')).toBeInTheDocument()
    })
    // The scope param must survive URL encoding of the `mcp:` prefix.
    expect(screen.getByTestId('probe').dataset.search).toBe('?scope=mcp%3Afilesystem')
  })

  // W1-1 (R2-P0-1(B)): url-only OAuth/HTTP installs show an honest Remote
  // state + hint (never the Offline bad state), the remote endpoint as the
  // preview, and a disabled Restart with an explanatory label.
  it('renders url-only servers as Remote with restart disabled', async () => {
    listMcpServers.mockResolvedValue([sampleRemote])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('Remote')).toBeInTheDocument()
    })
    // Honest hint, not the Offline badge.
    expect(
      screen.getByText('Remote server · Desktop support coming soon — use the CLI for now.'),
    ).toBeInTheDocument()
    expect(screen.queryByText('Offline')).not.toBeInTheDocument()
    // The remote endpoint is shown as the row preview.
    expect(screen.getByText('https://mcp.notion.com/mcp')).toBeInTheDocument()
    // Restart is disabled and its accessible name explains why.
    const restart = screen.getByRole('button', {
      name: "Remote servers can't be restarted from the desktop yet — use the CLI.",
    })
    expect(restart).toBeDisabled()
    // Remove still works on remote rows.
    expect(screen.getByText('Remove')).toBeEnabled()
  })

  // W1-7 (R2-P1-6): a failed stdio server shows the pool's concrete error,
  // not just a colour-only Offline pill.
  it('shows the concrete error of a failed server', async () => {
    listMcpServers.mockResolvedValue([sampleFailed])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('spawn /nonexistent ENOENT')).toBeInTheDocument()
    })
    expect(screen.getByText('Offline')).toBeInTheDocument()
  })

  it('closes modal on Escape key', async () => {
    listMcpServers.mockResolvedValue([])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /Add Server/ })).toBeInTheDocument()
    })
    fireEvent.click(screen.getByRole('button', { name: /Add Server/ }))
    await waitFor(() => {
      expect(screen.getByText('Add MCP Server')).toBeInTheDocument()
    })
    fireEvent.keyDown(document, { key: 'Escape' })
    await waitFor(() => {
      expect(screen.queryByText('Add MCP Server')).not.toBeInTheDocument()
    })
  })

  // W2-A (R4/A1): a connected pure remote row renders isomorphic to stdio —
  // an Online status pill and a *separate* tool-count chip — and its
  // Restart button is enabled (the pool restarts remote rows for real).
  it('renders a connected pure remote row like stdio with restart enabled', async () => {
    listMcpServers.mockResolvedValue([samplePureRemote])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('Online')).toBeInTheDocument()
    })
    // The tool count is its own element next to the status pill.
    expect(screen.getByText('3 tools')).toBeInTheDocument()
    expect(screen.getByText('https://mcp.deepwiki.com/mcp')).toBeInTheDocument()
    // Restart is NOT disabled for pure remote rows.
    const restart = screen.getByRole('button', { name: 'Restart deepwiki' })
    expect(restart).toBeEnabled()
    // No honest badge / hint on a wired pure remote row.
    expect(screen.queryByText('Remote')).not.toBeInTheDocument()
  })

  // W2-A honesty kept: the OAuth-product row still shows the W1-A Remote
  // badge + hint and a disabled restart; the header-less sibling would not.
  it('keeps the honest Remote badge for auth-gated url-only rows', async () => {
    listMcpServers.mockResolvedValue([sampleRemote])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('Remote')).toBeInTheDocument()
    })
    expect(
      screen.getByText('Remote server · Desktop support coming soon — use the CLI for now.'),
    ).toBeInTheDocument()
    const restart = screen.getByRole('button', {
      name: "Remote servers can't be restarted from the desktop yet — use the CLI.",
    })
    expect(restart).toBeDisabled()
  })

  // W2-A: a failed remote connection surfaces the pool's last_error inline,
  // same as failed stdio rows.
  it('shows the concrete error of a failed remote connection', async () => {
    listMcpServers.mockResolvedValue([sampleFailedRemote])
    renderWithRouter()
    await waitFor(() => {
      expect(
        screen.getByText("Remote MCP server 'flaky-remote' returned HTTP 500"),
      ).toBeInTheDocument()
    })
    expect(screen.getByText('Offline')).toBeInTheDocument()
  })

  // W2-A 顺手①: a disabled row shows a Disabled badge (not Offline) and an
  // inline switch; flipping it calls the toggle backend with the row name.
  it('shows Disabled badge and wires the inline enable toggle', async () => {
    listMcpServers.mockResolvedValue([sampleDisabled])
    setMcpServerEnabled.mockResolvedValue({ ...sampleDisabled, enabled: true })
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('Disabled')).toBeInTheDocument()
    })
    expect(screen.queryByText('Offline')).not.toBeInTheDocument()
    const toggle = screen.getByTestId('mcp-toggle-paused')
    expect(toggle.getAttribute('aria-checked')).toBe('false')
    fireEvent.click(toggle)
    await waitFor(() => {
      expect(setMcpServerEnabled).toHaveBeenCalledWith('paused', true)
    })
  })

  // W2-A 顺手①: disabling a running row through its switch.
  it('disables an enabled server through its inline switch', async () => {
    listMcpServers.mockResolvedValue([sampleInstalled])
    setMcpServerEnabled.mockResolvedValue({ ...sampleInstalled, enabled: false })
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByTestId('mcp-toggle-filesystem')).toBeInTheDocument()
    })
    fireEvent.click(screen.getByTestId('mcp-toggle-filesystem'))
    await waitFor(() => {
      expect(setMcpServerEnabled).toHaveBeenCalledWith('filesystem', false)
    })
    // The refresh re-reads the installed list.
    await waitFor(() => {
      expect(listMcpServers.mock.calls.length).toBeGreaterThanOrEqual(2)
    })
  })
})
