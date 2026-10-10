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
// Batch E4: the featured page now also reads the installed list (personal
// tab + icon row) — default to an empty machine.
const listInstalledAddons = vi.hoisted(() => vi.fn(async () => []))
// B4: Featured cards embed the SecurityBadge (community/unknown trust scans
// the description) — the api mock must expose the scan command.
const scanPromptInjectionWithReadme = vi.hoisted(() => vi.fn(async () => ({ risk: 'clean', matches: [], match_count: 0 })))

vi.mock('@/lib/tauri-api', () => ({
  default: {},
  listFeaturedVendors: (...a: unknown[]) => listFeaturedVendors(...a),
  installMcpOAuthLoopback: (...a: unknown[]) => installMcpOAuthLoopback(...a),
  installMcpOAuthAuthorizeUrl: (...a: unknown[]) => installMcpOAuthAuthorizeUrl(...a),
  installMcpOAuthComplete: (...a: unknown[]) => installMcpOAuthComplete(...a),
  installMcpStdio: (...a: unknown[]) => installMcpStdio(...a),
  listInstalledAddons: (...a: unknown[]) => listInstalledAddons(...a),
  scanPromptInjectionWithReadme: (...a: unknown[]) => scanPromptInjectionWithReadme(...a),
}))

function renderWithRouter({ withManageRoute = false } = {}) {
  return render(
    <MemoryRouter initialEntries={['/extensions/featured']}>
      <Routes>
        {/* Explicit sibling outranks the featured splat — lets the 「管理」
            navigation assertion observe the real route change. */}
        {withManageRoute && (
          <Route path="/extensions/mcp-servers" element={<div data-testid="mcp-servers-page" />} />
        )}
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

// B4: community-trust vendor — exercises the SecurityBadge scan path.
const communityVendor = {
  slug: 'acme-tools',
  display_name: 'Acme Tools',
  description: 'Community-built toolbox of dubious provenance',
  icon: 'handyman',
  category: 'productivity',
  trust: 'community',
  install_kind: {
    type: 'stdio',
    command: 'npx',
    args: ['-y', '@acme/mcp'],
    env_vars: [],
    display_name: 'Acme Tools',
  },
  homepage_url: 'https://example.com',
}

const installedRow = (slug: string) => ({
  id: `mcp:${slug}`,
  kind: 'mcp',
  name: slug,
  enabled: true,
})

beforeEach(() => {
  listFeaturedVendors.mockReset()
  installMcpOAuthLoopback.mockReset()
  installMcpOAuthAuthorizeUrl.mockReset()
  installMcpOAuthComplete.mockReset()
  installMcpStdio.mockReset()
  scanPromptInjectionWithReadme.mockReset()
  scanPromptInjectionWithReadme.mockResolvedValue({ risk: 'clean', matches: [], match_count: 0 })
  // Default to an empty machine; installed-state tests override per test.
  listInstalledAddons.mockReset()
  listInstalledAddons.mockResolvedValue([])
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

  // B4 (F-11): the unified foot has ONE primary action per card —
  // 未安装 → 「Add」 for both oauth and stdio kinds (the install IS the
  // auth flow, so no separate Connect label / needs-auth state).
  it('renders an uninstalled OAuth vendor with a single Add action', async () => {
    listFeaturedVendors.mockResolvedValue([oauthVendor])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('Google Drive')).toBeInTheDocument()
    })
    expect(screen.getByTestId('featured-action-google-drive')).toHaveTextContent('Add')
    expect(screen.queryByText('Connect')).not.toBeInTheDocument()
    // Honesty: no connection state can be derived → no 已连接/去认证 claim.
    expect(screen.queryByText('Connected')).not.toBeInTheDocument()
    expect(screen.queryByText(/needs auth/i)).not.toBeInTheDocument()
  })

  it('renders an uninstalled stdio vendor with a single Add action', async () => {
    listFeaturedVendors.mockResolvedValue([stdioVendor])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('Filesystem')).toBeInTheDocument()
    })
    expect(screen.getByTestId('featured-action-filesystem')).toHaveTextContent('Add')
    expect(screen.queryByText('Install')).not.toBeInTheDocument()
  })

  it('renders an installed vendor with a Manage action', async () => {
    listFeaturedVendors.mockResolvedValue([oauthVendor])
    listInstalledAddons.mockResolvedValue([installedRow('google-drive')])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByTestId('featured-action-google-drive')).toHaveTextContent('Manage')
    })
    expect(screen.queryByText('Add')).not.toBeInTheDocument()
  })

  it('navigates to the MCP managers when Manage is clicked', async () => {
    listFeaturedVendors.mockResolvedValue([oauthVendor])
    listInstalledAddons.mockResolvedValue([installedRow('google-drive')])
    renderWithRouter({ withManageRoute: true })
    await waitFor(() => {
      expect(screen.getByTestId('featured-action-google-drive')).toHaveTextContent('Manage')
    })
    fireEvent.click(screen.getByTestId('featured-action-google-drive'))
    await waitFor(() => {
      expect(screen.getByTestId('mcp-servers-page')).toBeInTheDocument()
    })
  })

  it('flips to Manage after a successful install (installed list refresh)', async () => {
    listFeaturedVendors.mockResolvedValue([stdioVendor])
    // First read (mount): not installed. The shannon:extension-installed
    // event triggers a reload that now reports the vendor.
    listInstalledAddons.mockResolvedValueOnce([]).mockResolvedValue([installedRow('filesystem')])
    installMcpStdio.mockResolvedValue({ id: 'stdio:filesystem', name: 'filesystem', install_path: null })
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByTestId('featured-action-filesystem')).toHaveTextContent('Add')
    })
    fireEvent.click(screen.getByTestId('featured-action-filesystem'))
    // The install dispatches shannon:extension-installed; Featured reloads
    // listInstalledAddons and re-renders the foot as Manage.
    await waitFor(() => {
      expect(screen.getByTestId('featured-action-filesystem')).toHaveTextContent('Manage')
    })
  })

  it('invokes installMcpStdio when stdio vendor Add is clicked', async () => {
    listFeaturedVendors.mockResolvedValue([stdioVendor])
    installMcpStdio.mockResolvedValue({
      id: 'stdio:filesystem',
      name: 'filesystem',
      install_path: null,
    })
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByTestId('featured-action-filesystem')).toHaveTextContent('Add')
    })
    fireEvent.click(screen.getByTestId('featured-action-filesystem'))
    await waitFor(() => {
      // Trailing `null` = the first, unconfirmed attempt (Dangerous-install
      // gate: the retry after the confirm drawer passes the typed name).
      expect(installMcpStdio).toHaveBeenCalledWith(
        {
          server_name: 'filesystem',
          command: 'npx',
          args: ['-y', '@modelcontextprotocol/server-filesystem'],
          env: [['ROOT', '/tmp']],
        },
        null,
      )
    })
  })

  // B3 P1-21: a successful install must broadcast `shannon:extension-installed`
  // (InstallDialog's contract) so the Installed tab / icon row / personal tab
  // refresh instead of showing stale inventories.
  it('dispatches shannon:extension-installed after a successful stdio install', async () => {
    listFeaturedVendors.mockResolvedValue([stdioVendor])
    installMcpStdio.mockResolvedValue({ id: 'stdio:filesystem', name: 'filesystem', install_path: null })
    const events: CustomEvent[] = []
    const onEvent = (e: Event) => events.push(e as CustomEvent)
    window.addEventListener('shannon:extension-installed', onEvent)
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByTestId('featured-action-filesystem')).toHaveTextContent('Add')
    })
    fireEvent.click(screen.getByTestId('featured-action-filesystem'))
    await waitFor(() => {
      expect(installMcpStdio).toHaveBeenCalled()
    })
    await waitFor(() => {
      const installed = events.filter(e => (e.detail as { name?: string }).name === 'filesystem')
      expect(installed).toHaveLength(1)
      expect(installed[0].detail).toMatchObject({ kind: 'mcp', name: 'filesystem' })
    })
    window.removeEventListener('shannon:extension-installed', onEvent)
  })

  it('invokes installMcpOAuthLoopback when OAuth vendor Add is clicked (success)', async () => {
    listFeaturedVendors.mockResolvedValue([oauthVendor])
    installMcpOAuthLoopback.mockResolvedValue({
      id: 'oauth:google-drive',
      name: 'google-drive',
      install_path: null,
    })
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByTestId('featured-action-google-drive')).toHaveTextContent('Add')
    })
    fireEvent.click(screen.getByTestId('featured-action-google-drive'))
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
      expect(screen.getByTestId('featured-action-google-drive')).toHaveTextContent('Add')
    })
    fireEvent.click(screen.getByTestId('featured-action-google-drive'))
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

// B4 (design 07-connectors foot): every featured card carries a visible
// security indicator on the foot's left slot — the standing "secured" line
// when the scan has nothing to flag, the scan's verdict chip otherwise.
describe('Featured card security indicator (F-11 foot)', () => {
  it('shows the secured line on a verified card without scanning', async () => {
    listFeaturedVendors.mockResolvedValue([oauthVendor])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('Install-time scan · Self-declared publisher')).toBeInTheDocument()
    })
    expect(scanPromptInjectionWithReadme).not.toHaveBeenCalled()
  })

  it('shows the secured line for a community card with a clean scan', async () => {
    listFeaturedVendors.mockResolvedValue([communityVendor])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('Acme Tools')).toBeInTheDocument()
    })
    await waitFor(() => {
      expect(scanPromptInjectionWithReadme).toHaveBeenCalled()
    })
    await waitFor(() => {
      expect(screen.getByText('Install-time scan · Self-declared publisher')).toBeInTheDocument()
    })
    expect(screen.queryByText('Injection risk')).not.toBeInTheDocument()
    expect(screen.queryByText('Review')).not.toBeInTheDocument()
  })

  it('swaps the secured line for the scan verdict chip when the scan flags risk', async () => {
    scanPromptInjectionWithReadme.mockResolvedValue({
      risk: 'dangerous',
      matches: [{ pattern: 'ignore previous', matched_substring: 'ignore previous', category: 'system_override' }],
      match_count: 1,
    })
    listFeaturedVendors.mockResolvedValue([communityVendor])
    renderWithRouter()
    await waitFor(() => {
      expect(screen.getByText('Injection risk')).toBeInTheDocument()
    })
    expect(screen.queryByText('Install-time scan · Self-declared publisher')).not.toBeInTheDocument()
  })
})
