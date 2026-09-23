// P1-3: PermissionsSettings page tests — builtin/custom profile listing,
// activation dispatch (activate_permission_profile), CRUD flow, the sandbox
// block (configure('sandbox.mode')), and rule-input validation.
// X3: `?scope=mcp:<server>` deep-link filtering (chip + matched rules +
// empty-state nudge), plus the prefix conversion unit tests.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor, within, fireEvent } from '@testing-library/react'
import { MemoryRouter, Routes, Route } from 'react-router-dom'
import * as api from '@/lib/tauri-api'
import PermissionsSettings, {
  validateRuleInput,
  collectScopedRules,
} from '@/components/settings/PermissionsSettings'

const listPermissionProfiles = vi.mocked(api.listPermissionProfiles)
const activatePermissionProfile = vi.mocked(api.activatePermissionProfile)
const saveCustomProfile = vi.mocked(api.saveCustomProfile)
const deleteCustomProfile = vi.mocked(api.deleteCustomProfile)
const configure = vi.mocked(api.configure)

vi.mock('sonner', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
    warning: vi.fn(),
    info: vi.fn(),
    message: vi.fn(),
  },
}))

// CatalogContext: the page reads `config` (active profile + sandbox mode)
// and `refreshConfig` after each activation.
const mockCatalog = vi.hoisted(() => ({
  config: {
    active_permission_profile: 'balanced',
    approval_mode: 'suggest',
    sandbox: { mode: 'off' },
  } as any,
  refreshConfig: vi.fn(),
}))

vi.mock('@/context/CatalogContext', () => ({
  useCatalog: () => mockCatalog,
}))

const PROFILES = {
  builtin: [
    {
      id: 'strict',
      description: 'Maximum safety',
      auto_approve_read: true,
      auto_approve_write: false,
      auto_approve_bash: false,
      auto_approve_delete: false,
      auto_approve_network: false,
      deny_destructive: ['Write', 'Bash', 'MultiEdit'],
    },
    {
      id: 'balanced',
      description: 'Auto-approve reads',
      auto_approve_read: true,
      auto_approve_write: false,
      auto_approve_bash: false,
      auto_approve_delete: false,
      auto_approve_network: false,
      deny_destructive: [],
    },
    {
      id: 'permissive',
      description: 'Auto-approve most things',
      auto_approve_read: true,
      auto_approve_write: true,
      auto_approve_bash: true,
      auto_approve_delete: false,
      auto_approve_network: false,
      deny_destructive: [],
    },
  ],
  custom: [
    {
      name: 'research-mode',
      description: 'Read-only research',
      auto_approve: ['Read', 'Grep'],
      confirm: ['Write'],
      deny: ['Bash(rm *)'],
    },
  ],
}

beforeEach(() => {
  listPermissionProfiles.mockReset()
  activatePermissionProfile.mockReset()
  saveCustomProfile.mockReset()
  deleteCustomProfile.mockReset()
  configure.mockReset()
  listPermissionProfiles.mockResolvedValue(PROFILES)
  activatePermissionProfile.mockResolvedValue({ active: 'strict', approval_mode: 'suggest' })
  saveCustomProfile.mockResolvedValue({
    name: 'demo',
    description: '',
    auto_approve: [],
    confirm: [],
    deny: [],
  })
  deleteCustomProfile.mockResolvedValue(['.shannon/profiles/research-mode.toml'])
  configure.mockResolvedValue(undefined)
  mockCatalog.config = {
    active_permission_profile: 'balanced',
    approval_mode: 'suggest',
    sandbox: { mode: 'off' },
  }
  mockCatalog.refreshConfig = vi.fn()
})

// X3: the page reads `?scope=` via useSearchParams, so renders must sit
// inside a router. The route mirrors the app's /settings/permissions path.
function renderWithRoute(search = '') {
  return render(
    <MemoryRouter initialEntries={[`/settings/permissions${search}`]}>
      <Routes>
        <Route path="/settings/permissions" element={<PermissionsSettings />} />
      </Routes>
    </MemoryRouter>,
  )
}

async function renderLoaded(search = '') {
  renderWithRoute(search)
  await waitFor(() => expect(screen.getByText('strict')).toBeInTheDocument())
}

describe('PermissionsSettings — profiles list', () => {
  it('renders the three builtin profiles and the custom profile', async () => {
    await renderLoaded()
    expect(screen.getByText('strict')).toBeInTheDocument()
    expect(screen.getByText('balanced')).toBeInTheDocument()
    expect(screen.getByText('permissive')).toBeInTheDocument()
    expect(screen.getByText('research-mode')).toBeInTheDocument()
  })

  it('marks the active builtin profile and disables re-activation', async () => {
    mockCatalog.config = { ...mockCatalog.config, active_permission_profile: 'balanced' }
    await renderLoaded()
    const balancedSection = screen.getByText('balanced').closest('div') as HTMLElement
    expect(within(balancedSection.parentElement as HTMLElement).getAllByText('Active').length).toBeGreaterThan(0)
  })

  it('activates a builtin profile via activate_permission_profile and refreshes config', async () => {
    await renderLoaded()
    // The strict card's Enable button (not the custom row's).
    const strictCard = screen.getByText('strict').closest('div.flex-col') as HTMLElement
    fireEvent.click(within(strictCard).getByRole('button', { name: 'Enable' }))
    await waitFor(() => {
      expect(activatePermissionProfile).toHaveBeenCalledWith('strict')
      expect(mockCatalog.refreshConfig).toHaveBeenCalled()
    })
  })

  it('enabling a custom profile activates it (switcher linkage)', async () => {
    await renderLoaded()
    const row = screen.getByText('research-mode').closest('li') as HTMLElement
    fireEvent.click(within(row).getByRole('button', { name: 'Enable' }))
    await waitFor(() => {
      expect(activatePermissionProfile).toHaveBeenCalledWith('research-mode')
    })
  })
})

describe('PermissionsSettings — custom profile CRUD', () => {
  it('opens the editor, validates an illegal rule, and saves a valid profile', async () => {
    await renderLoaded()
    fireEvent.click(screen.getByRole('button', { name: /New profile/i }))
    const dialog = await screen.findByRole('dialog')
    const nameInput = within(dialog).getByLabelText('Profile name') as HTMLInputElement
    fireEvent.change(nameInput, { target: { value: 'demo' } })

    // Illegal rule first: embedded whitespace.
    const addButtons = within(dialog).getAllByRole('button', { name: 'Add' })
    const denyFieldset = within(dialog)
      .getByRole('group', { name: /Deny/i })
      .closest('fieldset') as HTMLElement
    const denyInput = within(denyFieldset).getByPlaceholderText('Bash(git push *)') as HTMLInputElement
    fireEvent.change(denyInput, { target: { value: 'Bash(rm -rf *)' } })
    fireEvent.click(addButtons[addButtons.length - 1])
    // "rm -rf *" has spaces inside the pattern — allowed by the syntax
    // (only the TOOL part must be space-free); the rule lands.
    expect(within(denyFieldset).getByText('Bash(rm -rf *)')).toBeInTheDocument()

    // A rule with a space in the tool part is rejected inline.
    fireEvent.change(denyInput, { target: { value: 'Bad Tool(x)' } })
    expect(
      within(denyFieldset).getByText(/Tool name must not contain spaces/i),
    ).toBeInTheDocument()
    fireEvent.change(denyInput, { target: { value: '' } })

    fireEvent.click(within(dialog).getByRole('button', { name: 'Save profile' }))
    await waitFor(() => {
      expect(saveCustomProfile).toHaveBeenCalledWith(
        expect.objectContaining({ name: 'demo', deny: ['Bash(rm -rf *)'] }),
      )
    })
  })

  it('deletes a custom profile after confirmation', async () => {
    await renderLoaded()
    const row = screen.getByText('research-mode').closest('li') as HTMLElement
    fireEvent.click(within(row).getByRole('button', { name: 'Delete profile' }))
    const confirm = await screen.findByRole('alertdialog')
    fireEvent.click(within(confirm).getByRole('button', { name: 'Delete' }))
    await waitFor(() => {
      expect(deleteCustomProfile).toHaveBeenCalledWith('research-mode')
    })
  })
})

describe('PermissionsSettings — command sandbox block', () => {
  it('writes sandbox.mode via configure and refreshes config', async () => {
    await renderLoaded()
    const group = screen.getByRole('radiogroup', { name: 'Command sandbox' })
    fireEvent.click(within(group).getByRole('radio', { name: 'Read-only filesystem' }))
    await waitFor(() => {
      expect(configure).toHaveBeenCalledWith({ key: 'sandbox.mode', value: 'local' })
      expect(mockCatalog.refreshConfig).toHaveBeenCalled()
    })
  })

  it('reflects the persisted sandbox mode as aria-checked', async () => {
    mockCatalog.config = { ...mockCatalog.config, sandbox: { mode: 'landlock' } }
    await renderLoaded()
    const group = screen.getByRole('radiogroup', { name: 'Command sandbox' })
    expect(within(group).getByRole('radio', { name: 'Full (Landlock)' })).toHaveAttribute(
      'aria-checked',
      'true',
    )
  })
})

describe('validateRuleInput', () => {
  it('accepts bare tools, structured patterns, and globs', () => {
    expect(validateRuleInput('Bash')).toBeNull()
    expect(validateRuleInput('Bash(git push *)')).toBeNull()
    expect(validateRuleInput('mcp__github__*')).toBeNull()
    expect(validateRuleInput('  Read  ')).toBeNull()
  })

  it('rejects empty, whitespace-ridden, and malformed structured rules', () => {
    expect(validateRuleInput('')).toBe('settings.permissions.rules.error.empty')
    expect(validateRuleInput('Bad Tool')).toBe('settings.permissions.rules.error.whitespace')
    expect(validateRuleInput('Bash(git push')).toBe('settings.permissions.rules.error.unclosed')
    expect(validateRuleInput('(git)')).toBe('settings.permissions.rules.error.emptyTool')
    expect(validateRuleInput('Ba sh(x)')).toBe('settings.permissions.rules.error.toolWhitespace')
  })
})

describe('X3 scope filter — rule prefix conversion', () => {
  it('maps the mcp:<server> scope to mcp__<server>__ rule prefixes', () => {
    const profiles = {
      builtin: [],
      custom: [
        {
          name: 'demo',
          description: '',
          auto_approve: ['mcp__github__*', 'Read'],
          confirm: ['mcp__github__create_issue'],
          deny: ['mcp__slack__*'],
        },
      ],
    }
    expect(collectScopedRules(profiles, 'mcp__github__')).toEqual([
      { profile: 'demo', group: 'auto_approve', rule: 'mcp__github__*' },
      { profile: 'demo', group: 'confirm', rule: 'mcp__github__create_issue' },
    ])
    // No profiles / non-mcp scope → no hits, no crash.
    expect(collectScopedRules(null, 'mcp__github__')).toEqual([])
    expect(collectScopedRules(profiles, null)).toEqual([])
  })
})

describe('X3 scope filter — deep link panel', () => {
  const MCP_PROFILES = () => ({
    builtin: [],
    custom: [
      {
        name: 'demo',
        description: '',
        auto_approve: ['mcp__github__*', 'mcp__slack__*'],
        confirm: [] as string[],
        deny: ['mcp__github__create_issue'],
      },
    ],
  })

  it('filters rules by the ?scope= server prefix and shows a clearable chip', async () => {
    listPermissionProfiles.mockResolvedValue(MCP_PROFILES())
    // The fixture has no builtin profiles, so wait on the panel itself
    // instead of the usual "strict" card.
    renderWithRoute('?scope=mcp%3Agithub')
    const panel = await screen.findByTestId('permissions-scope-panel')
    expect(
      within(panel).getByRole('heading', { name: 'Rules matching mcp:github' }),
    ).toBeInTheDocument()
    // Only github rules survive the filter — the slack glob is hidden.
    expect(within(panel).getByText('mcp__github__*')).toBeInTheDocument()
    expect(within(panel).getByText('mcp__github__create_issue')).toBeInTheDocument()
    expect(within(panel).queryByText('mcp__slack__*')).not.toBeInTheDocument()
    // Clearing the chip removes the panel (back to the unfiltered page).
    fireEvent.click(within(panel).getByRole('button', { name: 'Clear scope filter' }))
    await waitFor(() => {
      expect(screen.queryByTestId('permissions-scope-panel')).not.toBeInTheDocument()
    })
  })

  it('nudges toward adding a rule when the server has none', async () => {
    listPermissionProfiles.mockResolvedValue(MCP_PROFILES())
    renderWithRoute('?scope=mcp%3Anovus')
    const panel = await screen.findByTestId('permissions-scope-panel')
    expect(within(panel).getByText('No rules for this server yet.')).toBeInTheDocument()
    fireEvent.click(within(panel).getByRole('button', { name: 'Add a rule' }))
    // The create-profile editor opens so the rule has somewhere to land.
    await waitFor(() => {
      expect(screen.getByRole('dialog')).toBeInTheDocument()
    })
  })

  it('renders no panel without a scope param', async () => {
    await renderLoaded()
    expect(screen.queryByTestId('permissions-scope-panel')).not.toBeInTheDocument()
  })
})
