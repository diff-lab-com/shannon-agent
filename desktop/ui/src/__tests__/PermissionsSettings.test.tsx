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
const getOutboundDefenseStatus = vi.mocked(api.getOutboundDefenseStatus)

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
  getOutboundDefenseStatus.mockReset()
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
  // S1.6 — lit outbound-defense default (both real guards armed).
  getOutboundDefenseStatus.mockResolvedValue({
    key_redaction: { mode: 'redact', audit_hits: 4 },
    injection_scan: { mode: 'audit', audit_hits: 2 },
    signature_check: { install_time_only: true, semantics: 'self-declared' },
  })
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
  await waitFor(() => expect(screen.getByText('Strict')).toBeInTheDocument())
}

describe('PermissionsSettings — profiles list', () => {
  it('renders the three builtin profiles and the custom profile', async () => {
    await renderLoaded()
    expect(screen.getByText('Strict')).toBeInTheDocument()
    expect(screen.getByText('Balanced')).toBeInTheDocument()
    expect(screen.getByText('Permissive')).toBeInTheDocument()
    expect(screen.getByText('research-mode')).toBeInTheDocument()
  })

  it('marks the active builtin profile and disables re-activation', async () => {
    mockCatalog.config = { ...mockCatalog.config, active_permission_profile: 'balanced' }
    await renderLoaded()
    const balancedSection = screen.getByText('Balanced').closest('div') as HTMLElement
    expect(within(balancedSection.parentElement as HTMLElement).getAllByText('Active').length).toBeGreaterThan(0)
  })

  it('activates a builtin profile via activate_permission_profile and refreshes config', async () => {
    await renderLoaded()
    // The strict card's Enable button (not the custom row's).
    const strictCard = screen.getByText('Strict').closest('div.flex-col') as HTMLElement
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

  it('renames via save + delete of the old file (P1-12)', async () => {
    await renderLoaded()
    fireEvent.click(within(screen.getByText('research-mode').closest('li') as HTMLElement)
      .getByRole('button', { name: 'Edit profile' }))
    const dialog = await screen.findByRole('dialog')
    const nameInput = within(dialog).getByLabelText('Profile name') as HTMLInputElement
    fireEvent.change(nameInput, { target: { value: 'renamed-mode' } })
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save profile' }))
    await waitFor(() => {
      // The new file is written first, then the stale one removed.
      expect(saveCustomProfile).toHaveBeenCalledWith(
        expect.objectContaining({ name: 'renamed-mode' }),
      )
      expect(deleteCustomProfile).toHaveBeenCalledWith('research-mode')
    })
    // The rename is not an activation — no profile switch is dispatched.
    expect(activatePermissionProfile).not.toHaveBeenCalled()
    await waitFor(() => expect(listPermissionProfiles.mock.calls.length).toBeGreaterThan(1))
  })

  it('renames the active profile without leaving a stale active pointer (P1-12)', async () => {
    mockCatalog.config = { ...mockCatalog.config, active_permission_profile: 'research-mode' }
    await renderLoaded()
    fireEvent.click(within(screen.getByText('research-mode').closest('li') as HTMLElement)
      .getByRole('button', { name: 'Edit profile' }))
    const dialog = await screen.findByRole('dialog')
    const nameInput = within(dialog).getByLabelText('Profile name') as HTMLInputElement
    fireEvent.change(nameInput, { target: { value: 'renamed-mode' } })
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save profile' }))
    await waitFor(() => {
      // Deactivated before the old file is deleted, re-activated under the
      // new name afterwards.
      expect(activatePermissionProfile).toHaveBeenNthCalledWith(1, null)
      expect(deleteCustomProfile).toHaveBeenCalledWith('research-mode')
      expect(activatePermissionProfile).toHaveBeenNthCalledWith(2, 'renamed-mode')
    })
    expect(mockCatalog.refreshConfig).toHaveBeenCalled()
  })

  it('renders an error state with retry when the profile list fails to load (P1-13)', async () => {
    listPermissionProfiles.mockRejectedValue(new Error('ipc down'))
    renderWithRoute()
    const alert = await screen.findByRole('alert')
    expect(within(alert).getByText("Couldn't load permission profiles")).toBeInTheDocument()
    // The builtin tiers are NOT rendered as an empty list.
    expect(screen.queryByText('strict')).not.toBeInTheDocument()
    expect(screen.queryByText('research-mode')).not.toBeInTheDocument()
    // Retry re-issues the load and recovers.
    listPermissionProfiles.mockResolvedValue(PROFILES)
    fireEvent.click(within(alert).getByRole('button', { name: 'Retry' }))
    await waitFor(() => expect(screen.getByText('Strict')).toBeInTheDocument())
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
    // Group labels must resolve to localized titles (auto_approve → "Auto-
    // approve", NOT the raw `…editor.auto_approve.title` key — regression
    // guard for the group-name/key-segment mismatch). The label shares a
    // span with the profile name, hence the regex match.
    expect(within(panel).getAllByText(/Auto-approve/).length).toBeGreaterThan(0)
    expect(within(panel).getAllByText(/Deny/).length).toBeGreaterThan(0)
    expect(
      within(panel).queryByText(/settings\.permissions\.editor\./),
    ).not.toBeInTheDocument()
    // Clearing the chip removes the panel (back to the unfiltered page).
    // 卡 3d wording: the chip only lifts the panel overlay on the
    // unfiltered page — "panel filter", not "scope filter".
    fireEvent.click(within(panel).getByRole('button', { name: 'Clear panel filter' }))
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

describe('S1.6 outbound-defense card — live status, honesty contract', () => {
  it('renders the lit state: modes, findings counts, and the static signature note', async () => {
    await renderLoaded()
    const card = screen.getByTestId('permissions-outbound-card')
    // Secret guard: release-default redact mode, 4 findings this run.
    const key = within(card).getByTestId('outbound-key-redaction-status')
    expect(key).toHaveTextContent('Redacting')
    expect(key).toHaveTextContent('Findings this run: 4')
    // Injection scanner: default audit mode, 2 findings this run.
    const injection = within(card).getByTestId('outbound-injection-scan-status')
    expect(injection).toHaveTextContent('Auditing')
    expect(injection).toHaveTextContent('Findings this run: 2')
    // The reconcile note is gone — the guards DO run today, so the card
    // must not claim they are still planned (reverse-distortion fix).
    expect(within(card).queryByText(/no engine capability backs them yet/i)).not.toBeInTheDocument()
    // Signature check: honest static capability note (install-time,
    // self-declared) — never a fabricated runtime state.
    expect(within(card).getByTestId('outbound-signature-note')).toHaveTextContent(
      'Install-time check · self-declared semantics',
    )
    // Read-only round: no switches anywhere in the card.
    expect(within(card).queryByRole('switch')).not.toBeInTheDocument()
  })

  it('refresh re-queries get_outbound_defense_status', async () => {
    await renderLoaded()
    expect(getOutboundDefenseStatus).toHaveBeenCalledTimes(1)
    fireEvent.click(screen.getByRole('button', { name: 'Refresh defense status' }))
    await waitFor(() => expect(getOutboundDefenseStatus).toHaveBeenCalledTimes(2))
  })

  it('renders unknown on command failure — never a fabricated zero or mode', async () => {
    getOutboundDefenseStatus.mockRejectedValue(new Error('backend unavailable'))
    await renderLoaded()
    const card = screen.getByTestId('permissions-outbound-card')
    const key = within(card).getByTestId('outbound-key-redaction-status')
    expect(key).toHaveTextContent('Status unknown')
    const injection = within(card).getByTestId('outbound-injection-scan-status')
    expect(injection).toHaveTextContent('Status unknown')
    // No count text anywhere in the two live rows, and no mode labels.
    expect(within(card).queryByText(/Findings this run:/)).not.toBeInTheDocument()
    expect(within(card).queryByText('Redacting')).not.toBeInTheDocument()
    expect(within(card).queryByText('Auditing')).not.toBeInTheDocument()
    // The failure note renders; the signature note stays static-honest.
    expect(within(card).getByText(/Defense status could not be read right now/)).toBeInTheDocument()
    expect(within(card).getByTestId('outbound-signature-note')).toHaveTextContent(
      'Install-time check · self-declared semantics',
    )
  })

  it('renders 未启用 for a null guard — distinct from zero findings', async () => {
    // Fresh process where no engine query has installed the guards yet.
    getOutboundDefenseStatus.mockResolvedValue({
      key_redaction: null,
      injection_scan: null,
      signature_check: { install_time_only: true, semantics: 'self-declared' },
    })
    await renderLoaded()
    const card = screen.getByTestId('permissions-outbound-card')
    expect(within(card).getByTestId('outbound-key-redaction-status')).toHaveTextContent('Not enabled')
    expect(within(card).getByTestId('outbound-injection-scan-status')).toHaveTextContent('Not enabled')
    // "Not enabled" is a real state — it must not carry a findings count.
    expect(within(card).queryByText(/Findings this run:/)).not.toBeInTheDocument()
  })

  it('renders an unknown future mode verbatim instead of guessing a label', async () => {
    getOutboundDefenseStatus.mockResolvedValue({
      key_redaction: { mode: 'future-mode', audit_hits: 1 },
      injection_scan: { mode: 'audit', audit_hits: 0 },
      signature_check: { install_time_only: true, semantics: 'self-declared' },
    })
    await renderLoaded()
    // A mode the UI does not know renders as itself — the raw value is the
    // only honest readout; zero findings stays a present zero.
    expect(screen.getByTestId('outbound-key-redaction-status')).toHaveTextContent('future-mode')
    expect(screen.getByTestId('outbound-injection-scan-status')).toHaveTextContent(
      'Findings this run: 0',
    )
  })
})
