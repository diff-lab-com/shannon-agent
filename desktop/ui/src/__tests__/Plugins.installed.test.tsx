// X6 — plugins page installed management + add-from-three-sources.
//
// Covers: the installed section (first real listPlugins consumer), the
// 「+ 添加插件」 menu (Git URL / local folder / archive), the SEC-1
// unverified-consent flow, enable/disable toggling, update (git rows only),
// the uninstall confirm (consent via inspected bundle listing, with the
// honest registry-only fallback when inspection fails), and the migration
// row's suppressed actions.
//
// Marketplace browsing itself stays covered by Plugins.marketplace.test.tsx.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor, fireEvent, within } from '@testing-library/react'
import Plugins from '@/components/extensions/Plugins'
import type { PluginInfo } from '@/lib/tauri-api'
import * as api from '@/lib/tauri-api'
import { open as openDialog } from '@tauri-apps/plugin-dialog'

vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() },
}))

import { toast } from 'sonner'

import type * as ReactRouterDom from 'react-router-dom'
vi.mock('react-router-dom', async () => {
  const actual = await vi.importActual<typeof ReactRouterDom>('react-router-dom')
  return {
    ...actual,
    useOutletContext: () => ({ search: (globalThis as { __PLUGINS_SEARCH__?: string }).__PLUGINS_SEARCH__ ?? '' }),
    useNavigate: () => () => {},
  }
})

const plugin = (overrides: Partial<PluginInfo> = {}): PluginInfo => ({
  name: 'web-plugin',
  version: '1.2.0',
  description: 'A git-installed plugin.',
  author: 'someone',
  plugin_type: 'skill',
  enabled: true,
  path: '/home/u/.shannon/plugins/web-plugin',
  source_format: 'shannon-toml',
  source: 'git',
  migration_imported: false,
  ...overrides,
})

const migrationRow = plugin({
  name: 'imported-claude-code',
  version: '1.0.0',
  description: 'Migration import from claude-code.',
  author: 'migration',
  path: '~/.config/shannon/plugins/imported-claude-code',
  source_format: 'claude-json',
  source: 'migration',
  migration_imported: true,
})

const localRow = plugin({
  name: 'folder-plugin',
  version: '0.4.0',
  description: 'A locally copied plugin.',
  enabled: false,
  path: '/home/u/.shannon/plugins/folder-plugin',
  source_format: 'claude-json',
  source: 'local',
})

function renderPlugins() {
  return render(<Plugins />)
}

async function openAddMenu() {
  fireEvent.click(screen.getByTestId('add-plugin-button'))
  await waitFor(() => expect(screen.getByRole('menu')).toBeInTheDocument())
}

beforeEach(() => {
  vi.mocked(api.listPlugins).mockReset().mockResolvedValue([])
  vi.mocked(api.installPlugin).mockReset().mockResolvedValue({ name: 'plugin-x', warnings: [] })
  vi.mocked(api.installPluginFromGit).mockReset().mockResolvedValue({ name: 'plugin-git', warnings: [] })
  vi.mocked(api.uninstallPlugin).mockReset().mockResolvedValue({ warnings: [] })
  vi.mocked(api.enablePlugin).mockReset().mockResolvedValue({ warnings: [] })
  vi.mocked(api.disablePlugin).mockReset().mockResolvedValue({ warnings: [] })
  vi.mocked(api.updatePlugin).mockReset().mockResolvedValue({ warnings: [] })
  vi.mocked(api.inspectPluginSource).mockReset().mockResolvedValue({
    name: 'preview-plugin',
    source_format: 'claude-json',
    skills: [],
    agents: [],
    commands: [],
    mcp_servers: [],
  })
  vi.mocked(openDialog).mockReset().mockResolvedValue(null)
  vi.mocked(toast.success).mockClear()
  vi.mocked(toast.error).mockClear()
  vi.mocked(toast.warning).mockClear()
  ;(globalThis as { __PLUGINS_SEARCH__?: string }).__PLUGINS_SEARCH__ = ''
})

describe('Plugins — installed section (X6)', () => {
  it('renders installed rows with name, version, source badge, and source_format', async () => {
    vi.mocked(api.listPlugins).mockResolvedValue([plugin(), localRow, migrationRow])
    renderPlugins()
    await waitFor(() => expect(screen.getByTestId('installed-row-web-plugin')).toBeInTheDocument())
    expect(screen.getByTestId('installed-row-folder-plugin')).toBeInTheDocument()
    expect(screen.getByTestId('installed-row-imported-claude-code')).toBeInTheDocument()
    const section = screen.getByTestId('plugins-installed-section')
    expect(within(section).getByText('Git')).toBeInTheDocument()
    expect(within(section).getByText('Local')).toBeInTheDocument()
    expect(within(section).getByText('Migration import')).toBeInTheDocument()
    expect(within(section).getByText('shannon-toml')).toBeInTheDocument()
    expect(within(section).getAllByText('claude-json')).toHaveLength(2)
    expect(within(section).getByText('3 plugins')).toBeInTheDocument()
  })

  it('shows the empty state when nothing is installed', async () => {
    vi.mocked(api.listPlugins).mockResolvedValue([])
    renderPlugins()
    await waitFor(() => expect(screen.getByText('Nothing installed yet')).toBeInTheDocument())
  })

  it('shows an error state when listPlugins rejects', async () => {
    vi.mocked(api.listPlugins).mockRejectedValue(new Error('boom'))
    renderPlugins()
    await waitFor(() => expect(screen.getByText('Could not load installed plugins.')).toBeInTheDocument())
  })

  it('refreshes the installed list when shannon:extension-installed fires', async () => {
    vi.mocked(api.listPlugins).mockResolvedValueOnce([]).mockResolvedValue([plugin()])
    renderPlugins()
    await waitFor(() => expect(screen.getByText('Nothing installed yet')).toBeInTheDocument())
    window.dispatchEvent(new CustomEvent('shannon:extension-installed', { detail: { kind: 'plugin', name: 'x' } }))
    await waitFor(() => expect(screen.getByTestId('installed-row-web-plugin')).toBeInTheDocument())
  })

  it('calls disablePlugin for an enabled row and enablePlugin for a disabled row', async () => {
    vi.mocked(api.listPlugins).mockResolvedValue([plugin(), localRow])
    renderPlugins()
    await waitFor(() => expect(screen.getByTestId('installed-row-web-plugin')).toBeInTheDocument())

    fireEvent.click(screen.getByLabelText('Enable or disable web-plugin'))
    await waitFor(() => expect(api.disablePlugin).toHaveBeenCalledWith('web-plugin'))

    await waitFor(() => expect(screen.getByLabelText('Enable or disable folder-plugin')).toBeInTheDocument())
    fireEvent.click(screen.getByLabelText('Enable or disable folder-plugin'))
    await waitFor(() => expect(api.enablePlugin).toHaveBeenCalledWith('folder-plugin'))
  })

  it('surfaces lifecycle warnings as a warning toast', async () => {
    vi.mocked(api.disablePlugin).mockResolvedValue({ warnings: ['could not remove ~/.shannon/skills/wp'] })
    vi.mocked(api.listPlugins).mockResolvedValue([plugin()])
    renderPlugins()
    await waitFor(() => expect(screen.getByTestId('installed-row-web-plugin')).toBeInTheDocument())

    fireEvent.click(screen.getByLabelText('Enable or disable web-plugin'))
    await waitFor(() =>
      expect(toast.warning).toHaveBeenCalledWith('could not remove ~/.shannon/skills/wp'),
    )
  })

  it('offers update only on git-sourced rows and calls updatePlugin', async () => {
    vi.mocked(api.listPlugins).mockResolvedValue([plugin(), localRow])
    renderPlugins()
    await waitFor(() => expect(screen.getByTestId('installed-row-web-plugin')).toBeInTheDocument())

    // git row → 更新 present and wired
    expect(screen.getByTestId('installed-update-web-plugin')).toBeInTheDocument()
    fireEvent.click(screen.getByTestId('installed-update-web-plugin'))
    await waitFor(() => expect(api.updatePlugin).toHaveBeenCalledWith('web-plugin'))

    // local row → update is a git pull that could never succeed; not offered
    expect(screen.queryByTestId('installed-update-folder-plugin')).not.toBeInTheDocument()
  })

  it('uninstall confirms via the inspected bundle listing, then uninstalls', async () => {
    vi.mocked(api.inspectPluginSource).mockResolvedValue({
      name: 'web-plugin',
      source_format: 'shannon-toml',
      skills: ['clip'],
      agents: ['reviewer.md'],
      commands: [],
      mcp_servers: ['filesystem'],
    })
    vi.mocked(api.uninstallPlugin).mockResolvedValue({ warnings: [] })
    vi.mocked(api.listPlugins).mockResolvedValue([plugin()])
    renderPlugins()
    await waitFor(() => expect(screen.getByTestId('installed-row-web-plugin')).toBeInTheDocument())

    fireEvent.click(screen.getByTestId('installed-uninstall-web-plugin'))

    // the preview inspects the plugin's own directory
    await waitFor(() =>
      expect(api.inspectPluginSource).toHaveBeenCalledWith('/home/u/.shannon/plugins/web-plugin'),
    )
    await waitFor(() => expect(screen.getByTestId('uninstall-list')).toBeInTheDocument())
    expect(screen.getByTestId('uninstall-skills')).toHaveTextContent('1 skills: clip')
    expect(screen.getByTestId('uninstall-agents')).toHaveTextContent('1 agents: reviewer.md')
    // empty groups are not invented
    expect(screen.queryByTestId('uninstall-commands')).not.toBeInTheDocument()
    expect(screen.getByTestId('uninstall-mcp')).toHaveTextContent('MCP servers: filesystem')

    fireEvent.click(screen.getByTestId('uninstall-confirm-button'))
    await waitFor(() => expect(api.uninstallPlugin).toHaveBeenCalledWith('web-plugin'))
    // list refreshed after the mutation
    await waitFor(() => expect(api.listPlugins).toHaveBeenCalledTimes(2))
  })

  it('says removal is registry-only when the bundle preview fails', async () => {
    vi.mocked(api.inspectPluginSource).mockRejectedValue(new Error('unreadable'))
    vi.mocked(api.listPlugins).mockResolvedValue([plugin()])
    renderPlugins()
    await waitFor(() => expect(screen.getByTestId('installed-row-web-plugin')).toBeInTheDocument())

    fireEvent.click(screen.getByTestId('installed-uninstall-web-plugin'))
    await waitFor(() => expect(screen.getByTestId('uninstall-inspect-failed')).toBeInTheDocument())
    expect(screen.getByTestId('uninstall-inspect-failed')).toHaveTextContent(
      'Only the registry entry will be removed',
    )

    // consent stays possible — honest, not blocking
    fireEvent.click(screen.getByTestId('uninstall-confirm-button'))
    await waitFor(() => expect(api.uninstallPlugin).toHaveBeenCalledWith('web-plugin'))
  })

  it('surfaces uninstall warnings from the lifecycle result', async () => {
    vi.mocked(api.uninstallPlugin).mockResolvedValue({ warnings: ['no materialized.json sidecar — registry-only uninstall'] })
    vi.mocked(api.listPlugins).mockResolvedValue([plugin()])
    renderPlugins()
    await waitFor(() => expect(screen.getByTestId('installed-row-web-plugin')).toBeInTheDocument())

    fireEvent.click(screen.getByTestId('installed-uninstall-web-plugin'))
    await waitFor(() => expect(screen.getByTestId('uninstall-list')).toBeInTheDocument())
    fireEvent.click(screen.getByTestId('uninstall-confirm-button'))
    await waitFor(() =>
      expect(toast.warning).toHaveBeenCalledWith('no materialized.json sidecar — registry-only uninstall'),
    )
  })
})

describe('Plugins — migration rows suppress destructive actions (X6)', () => {
  it('disables the switch and uninstall, hides update, and explains why', async () => {
    vi.mocked(api.listPlugins).mockResolvedValue([migrationRow])
    renderPlugins()
    await waitFor(() => expect(screen.getByTestId('installed-row-imported-claude-code')).toBeInTheDocument())

    // Base UI Switch uses aria-disabled rather than the disabled attribute.
    const toggle = screen.getByLabelText('Enable or disable imported-claude-code')
    expect(toggle).toHaveAttribute('aria-disabled', 'true')

    const uninstall = screen.getByTestId('installed-uninstall-imported-claude-code')
    expect(uninstall).toBeDisabled()
    expect(uninstall.closest('span')).toHaveAttribute('title', expect.stringContaining('Migration import record'))

    // no 更新 on a migration record
    expect(screen.queryByTestId('installed-update-imported-claude-code')).not.toBeInTheDocument()

    // nothing destructive can be invoked from the row
    expect(api.disablePlugin).not.toHaveBeenCalled()
    expect(api.uninstallPlugin).not.toHaveBeenCalled()
  })
})

describe('Plugins — add plugin from three sources (X6)', () => {
  it('installs from a Git URL via the dialog with default consent', async () => {
    vi.mocked(api.listPlugins).mockResolvedValue([])
    renderPlugins()
    await openAddMenu()

    fireEvent.click(screen.getByRole('menuitem', { name: 'From Git URL…' }))
    await waitFor(() => expect(screen.getByTestId('add-git-url-input')).toBeInTheDocument())

    fireEvent.change(screen.getByTestId('add-git-url-input'), {
      target: { value: 'https://github.com/u/plugin.git' },
    })
    fireEvent.click(screen.getByTestId('add-git-install'))

    await waitFor(() =>
      expect(api.installPluginFromGit).toHaveBeenCalledWith('https://github.com/u/plugin.git', false),
    )
    expect(toast.success).toHaveBeenCalledWith('Installed plugin-git.')
  })

  it('refuses empty URLs without invoking the backend', async () => {
    vi.mocked(api.listPlugins).mockResolvedValue([])
    renderPlugins()
    await openAddMenu()
    fireEvent.click(screen.getByRole('menuitem', { name: 'From Git URL…' }))
    await waitFor(() => expect(screen.getByTestId('add-git-url-input')).toBeInTheDocument())

    fireEvent.click(screen.getByTestId('add-git-install'))
    await waitFor(() => expect(screen.getByTestId('add-git-needs-url')).toBeInTheDocument())
    expect(api.installPluginFromGit).not.toHaveBeenCalled()
  })

  it('offers the explicit unverified opt-in after an SEC-1 refusal', async () => {
    vi.mocked(api.listPlugins).mockResolvedValue([])
    vi.mocked(api.installPluginFromGit).mockRejectedValueOnce(
      new Error('refused: manifest declares no permissions (set allow_unverified to override)'),
    )
    renderPlugins()
    await openAddMenu()
    fireEvent.click(screen.getByRole('menuitem', { name: 'From Git URL…' }))
    await waitFor(() => expect(screen.getByTestId('add-git-url-input')).toBeInTheDocument())

    fireEvent.change(screen.getByTestId('add-git-url-input'), {
      target: { value: 'https://github.com/u/shady.git' },
    })
    fireEvent.click(screen.getByTestId('add-git-install'))

    await waitFor(() => expect(screen.getByTestId('add-git-unverified-warning')).toBeInTheDocument())
    expect(toast.error).not.toHaveBeenCalled()

    fireEvent.click(screen.getByTestId('add-git-install-unverified'))
    await waitFor(() =>
      expect(api.installPluginFromGit).toHaveBeenCalledWith('https://github.com/u/shady.git', true),
    )
  })

  // Review fix (SEC-1): the refusal arms the opt-in for ONE remote. Editing
  // the URL aims the install at a different repo, so the armed consent must
  // reset — the opt-in stays hidden until the new remote earns its own
  // refusal.
  it('withdraws the unverified opt-in when the URL is edited after a refusal', async () => {
    vi.mocked(api.listPlugins).mockResolvedValue([])
    vi.mocked(api.installPluginFromGit).mockRejectedValueOnce(
      new Error('refused: manifest declares no permissions (set allow_unverified to override)'),
    )
    renderPlugins()
    await openAddMenu()
    fireEvent.click(screen.getByRole('menuitem', { name: 'From Git URL…' }))
    await waitFor(() => expect(screen.getByTestId('add-git-url-input')).toBeInTheDocument())

    fireEvent.change(screen.getByTestId('add-git-url-input'), {
      target: { value: 'https://github.com/u/shady.git' },
    })
    fireEvent.click(screen.getByTestId('add-git-install'))
    await waitFor(() => expect(screen.getByTestId('add-git-unverified-warning')).toBeInTheDocument())
    expect(screen.getByTestId('add-git-install-unverified')).toBeInTheDocument()

    // A different URL withdraws the armed consent.
    fireEvent.change(screen.getByTestId('add-git-url-input'), {
      target: { value: 'https://github.com/u/other.git' },
    })
    expect(screen.queryByTestId('add-git-install-unverified')).not.toBeInTheDocument()
    expect(screen.queryByTestId('add-git-unverified-warning')).not.toBeInTheDocument()
    // The default install button re-enables (it is disabled while armed).
    expect(screen.getByTestId('add-git-install')).not.toBeDisabled()

    // Re-typing the refused URL does NOT re-arm — only a fresh backend
    // refusal may. (The one-shot mock now resolves, so the default install
    // path goes through; the opt-in must still never appear.)
    fireEvent.change(screen.getByTestId('add-git-url-input'), {
      target: { value: 'https://github.com/u/shady.git' },
    })
    expect(screen.queryByTestId('add-git-install-unverified')).not.toBeInTheDocument()
  })

  it('installs from a local directory picked via the directory dialog', async () => {
    vi.mocked(api.listPlugins).mockResolvedValue([])
    vi.mocked(openDialog).mockResolvedValue('/home/u/my-plugin')
    renderPlugins()
    await openAddMenu()

    fireEvent.click(screen.getByRole('menuitem', { name: 'From local folder…' }))

    await waitFor(() => expect(api.installPlugin).toHaveBeenCalledWith('/home/u/my-plugin'))
    expect(toast.success).toHaveBeenCalledWith('Installed plugin-x.')
  })

  it('installs from a .dxt/.mcpb/.zip archive picked via the file dialog', async () => {
    vi.mocked(api.listPlugins).mockResolvedValue([])
    vi.mocked(openDialog).mockResolvedValue('/home/u/Downloads/bundle.mcpb')
    renderPlugins()
    await openAddMenu()

    fireEvent.click(screen.getByRole('menuitem', { name: 'From archive (.dxt / .mcpb / .zip)…' }))

    await waitFor(() => expect(api.installPlugin).toHaveBeenCalledWith('/home/u/Downloads/bundle.mcpb'))
    // the picker was constrained to plugin archive extensions
    expect(openDialog).toHaveBeenCalledWith(
      expect.objectContaining({
        multiple: false,
        filters: [expect.objectContaining({ extensions: ['dxt', 'mcpb', 'zip'] })],
      }),
    )
  })

  it('does not install when the picker is cancelled', async () => {
    vi.mocked(api.listPlugins).mockResolvedValue([])
    vi.mocked(openDialog).mockResolvedValue(null)
    renderPlugins()
    await openAddMenu()

    fireEvent.click(screen.getByRole('menuitem', { name: 'From local folder…' }))

    await waitFor(() => expect(screen.queryByRole('menu')).not.toBeInTheDocument())
    expect(api.installPlugin).not.toHaveBeenCalled()
  })

  it('surfaces install warnings as a warning toast', async () => {
    vi.mocked(api.listPlugins).mockResolvedValue([])
    vi.mocked(openDialog).mockResolvedValue('/home/u/my-plugin')
    vi.mocked(api.installPlugin).mockResolvedValue({ name: 'plugin-x', warnings: ['sse-only mcp server skipped'] })
    renderPlugins()
    await openAddMenu()

    fireEvent.click(screen.getByRole('menuitem', { name: 'From local folder…' }))

    await waitFor(() => expect(toast.warning).toHaveBeenCalledWith('sse-only mcp server skipped'))
  })
})
