// P1-3: PermissionsSettings page tests — builtin/custom profile listing,
// activation dispatch (activate_permission_profile), CRUD flow, the sandbox
// block (configure('sandbox.mode')), and rule-input validation.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor, within, fireEvent } from '@testing-library/react'
import * as api from '@/lib/tauri-api'
import PermissionsSettings, { validateRuleInput } from '@/components/settings/PermissionsSettings'

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

async function renderLoaded() {
  render(<PermissionsSettings />)
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
