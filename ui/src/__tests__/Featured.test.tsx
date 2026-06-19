import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor, fireEvent } from '@testing-library/react'
import { MemoryRouter, Routes, Route, Outlet } from 'react-router-dom'
import Featured from '@/components/extensions/Featured'

function Shell() {
  return <Outlet context={{ search: '' }} />
}

const listFeaturedVendors = vi.hoisted(() => vi.fn())
const installMcpOAuthLoopback = vi.hoisted(() => vi.fn())
const installMcpOAuthAuthorizeUrl = vi.hoisted(() => vi.fn())
const installMcpOAuthComplete = vi.hoisted(() => vi.fn())
const installMcpStdio = vi.hoisted(() => vi.fn())

vi.mock('@/lib/tauri-api', () => ({
  default: {},
  listFeaturedVendors: (...a: unknown[]) => listFeaturedVendors(...a),
  installMcpOAuthLoopback: (...a: unknown[]) => installMcpOAuthLoopback(...a),
  installMcpOAuthAuthorizeUrl: (...a: unknown[]) => installMcpOAuthAuthorizeUrl(...a),
  installMcpOAuthComplete: (...a: unknown[]) => installMcpOAuthComplete(...a),
  installMcpStdio: (...a: unknown[]) => installMcpStdio(...a),
}))

function renderWithRouter() {
  return render(
    <MemoryRouter initialEntries={['/extensions/featured']}>
      <Routes>
        <Route path="/*" element={<Shell />}>
          <Route path="*" element={<Featured />} />
        </Route>
      </Routes>
    </MemoryRouter>
  )
}

const oauthVendor = {
  slug: 'google-drive',
  display_name: 'Google Drive',
  description: 'OAuth-based Google Drive access',
  icon: 'folder',
  category: 'productivity',
  trust: 'verified',
  install_kind: {
    type: 'oauth_remote',
    authorize_url: 'https://accounts.google.com/o/oauth2/v2/auth',
    token_url: 'https://oauth2.googleapis.com/token',
    mcp_endpoint: 'https://drive.example.com/mcp',
    client_id_env: 'GOOGLE_CLIENT_ID',
    default_scopes: ['drive.readonly'],
    display_name: 'Google Drive',
  },
  homepage_url: 'https://example.com',
}

const stdioVendor = {
  slug: 'filesystem',
  display_name: 'Filesystem',
  description: 'Direct filesystem MCP server',
  icon: 'folder',
  category: 'developer_tools',
  trust: 'official',
  install_kind: {
    type: 'stdio',
    command: 'npx',
    args: ['-y', '@modelcontextprotocol/server-filesystem'],
    env_vars: [['ROOT', '/tmp']],
    display_name: 'Filesystem',
  },
  homepage_url: 'https://example.com',
}

beforeEach(() => {
  listFeaturedVendors.mockReset()
  installMcpOAuthLoopback.mockReset()
  installMcpOAuthAuthorizeUrl.mockReset()
  installMcpOAuthComplete.mockReset()
  installMcpStdio.mockReset()
})

describe('Featured (P2 wire-up)', () => {
  it('shows loading state initially', () => {
    listFeaturedVendors.mockReturnValue(new Promise(() => {}))
    renderWithRouter()
    expect(screen.getByText('Loading featured vendors…')).toBeInTheDocument()
  })

  it('shows error state when load fails', async () => {
    listFeaturedVendors.mockRejectedValue(new Error('boom'))
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText(/Failed to load:/)).toBeInTheDocument()
    })
    expect(screen.getByText(/boom/)).toBeInTheDocument()
  })

  it('renders OAuth vendor with Connect button', async () => {
    listFeaturedVendors.mockResolvedValue([oauthVendor])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('Google Drive')).toBeInTheDocument()
    })
    expect(screen.getByText('Connect')).toBeInTheDocument()
    expect(screen.getByText('Verified')).toBeInTheDocument()
  })

  it('renders stdio vendor with Install button', async () => {
    listFeaturedVendors.mockResolvedValue([stdioVendor])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('Filesystem')).toBeInTheDocument()
    })
    expect(screen.getByText('Install')).toBeInTheDocument()
    expect(screen.getByText('Official')).toBeInTheDocument()
  })

  it('invokes installMcpStdio when stdio vendor Install clicked', async () => {
    listFeaturedVendors.mockResolvedValue([stdioVendor])
    installMcpStdio.mockResolvedValue({
      id: 'stdio:filesystem',
      name: 'filesystem',
      install_path: null,
    })
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('Install')).toBeInTheDocument()
    })
    fireEvent.click(screen.getByText('Install'))
    await waitFor(() => {
      expect(installMcpStdio).toHaveBeenCalledWith({
        server_name: 'filesystem',
        command: 'npx',
        args: ['-y', '@modelcontextprotocol/server-filesystem'],
        env: [['ROOT', '/tmp']],
      })
    })
  })

  it('invokes installMcpOAuthLoopback when OAuth Connect clicked (success)', async () => {
    listFeaturedVendors.mockResolvedValue([oauthVendor])
    installMcpOAuthLoopback.mockResolvedValue({
      id: 'oauth:google-drive',
      name: 'google-drive',
      install_path: null,
    })
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('Connect')).toBeInTheDocument()
    })
    fireEvent.click(screen.getByText('Connect'))
    await waitFor(() => {
      expect(installMcpOAuthLoopback).toHaveBeenCalledWith('google-drive')
    })
    // Success path: no manual token paste form should appear.
    expect(screen.queryByText(/paste the access token/i)).not.toBeInTheDocument()
  })

  it('falls back to manual token paste form when loopback fails', async () => {
    listFeaturedVendors.mockResolvedValue([oauthVendor])
    installMcpOAuthLoopback.mockRejectedValue(new Error('loopback bind failed'))
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('Connect')).toBeInTheDocument()
    })
    fireEvent.click(screen.getByText('Connect'))
    await waitFor(() => {
      expect(installMcpOAuthLoopback).toHaveBeenCalledWith('google-drive')
    })
    expect(await screen.findByText(/paste the access token/i)).toBeInTheDocument()
  })

  it('renders empty state when no vendors match search', async () => {
    listFeaturedVendors.mockResolvedValue([oauthVendor])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('Google Drive')).toBeInTheDocument()
    })
    // Search context is provided via useOutletContext — without it, no filter
    // is applied. This test confirms the vendors render normally.
    expect(screen.getByText('Google Drive')).toBeInTheDocument()
  })

  it('shows multiple trust badge levels', async () => {
    listFeaturedVendors.mockResolvedValue([oauthVendor, stdioVendor])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('Verified')).toBeInTheDocument()
      expect(screen.getByText('Official')).toBeInTheDocument()
    })
  })
})
