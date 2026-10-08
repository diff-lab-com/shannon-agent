// GB P2-4 — composer approval-tier switcher tests; rewritten for the Aurora
// 2026-10 four-stop segmented control (裁决 B1, design 02:256-261).
//
// Pins the composer↔settings sync contract: the segmented control renders
// the SHARED table's four stops (ask / auto-edit / plan / full-auto — `plan`
// is a legal engine approval_mode), marks the active one from
// `config.approval_mode` (legacy values normalize), switching commits the
// same configure('approval_mode') write the General page performs, the
// High-risk note rides the group title, and out-of-ladder engine values
// surface through the honest raw badge instead of pretending to be a tier.

import { describe, it, expect, vi, beforeEach, beforeAll, afterAll } from 'vitest'
import { render, screen, fireEvent, waitFor, within } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import ChatInput from '@/components/chat/ChatInput'
import * as api from '@/lib/tauri-api'
import type * as ReactRouterDom from 'react-router-dom'

vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn(), message: vi.fn() },
}))

vi.mock('react-router-dom', async () => {
  const actual = await vi.importActual<typeof ReactRouterDom>('react-router-dom')
  return {
    ...actual,
    useOutletContext: () => ({ search: '' }),
    useNavigate: () => () => {},
  }
})

const mockRefreshConfig = vi.fn()
const mockRefreshStatus = vi.fn()

function makeConfig(approvalMode: string) {
  return {
    approval_mode: approvalMode,
    model: 'claude-sonnet-4-6',
    provider: 'anthropic',
    working_dir: '/home/user/projects',
  }
}

let configOverride = makeConfig('suggest')

vi.mock('@/context/CatalogContext', () => ({
  useCatalog: () => ({
    get config() {
      return configOverride
    },
    status: { model: 'Claude Sonnet 4.6', provider: 'anthropic', querying: false, message_count: 0, working_dir: '/home/user/projects' },
    models: [
      { id: 'anthropic-claude-sonnet-4-6', name: 'Claude Sonnet 4.6', provider: 'anthropic', context_window: 200000 },
    ],
    refreshConfig: mockRefreshConfig,
    refreshStatus: mockRefreshStatus,
  }),
}))

vi.mock('@/context/SessionContext', () => ({
  useSessions: () => ({ currentSessionId: 'sess-1' }),
}))

vi.mock('@/lib/tauri-api', async () => {
  const actual = await vi.importActual<object>('@/lib/tauri-api')
  return {
    ...actual,
    configure: vi.fn().mockResolvedValue(undefined),
    setSessionModel: vi.fn().mockResolvedValue(undefined),
    clearSessionModel: vi.fn().mockResolvedValue(undefined),
    getSessionModel: vi.fn().mockResolvedValue(null),
    getSessionContextBreakdown: vi.fn().mockResolvedValue({
      totalTokens: 0, contextWindow: null,
      categories: [{ key: 'system', tokens: 0 }, { key: 'tools', tokens: 0 }, { key: 'skills', tokens: 0 },
        { key: 'memory', tokens: 0 }, { key: 'mcp', tokens: 0 }, { key: 'conversation', tokens: 0 }],
    }),
    getSessionUsage: vi.fn().mockResolvedValue({ cost_usd: 0 }),
    getSessionBudget: vi.fn().mockResolvedValue(null),
    onWebviewFileDrop: vi.fn().mockResolvedValue(() => {}),
  }
})

function renderChatInput(props: Partial<React.ComponentProps<typeof ChatInput>> = {}) {
  const defaultProps = {
    value: '',
    onChange: vi.fn(),
    onSend: vi.fn(),
    onExecuteSlash: vi.fn(),
    attachedFiles: [],
    onAttach: vi.fn(),
    onDetachAll: vi.fn(),
    isQuerying: false,
    onCancelQuery: vi.fn(),
    onOpenQuickFix: vi.fn(),
    onOpenEditor: vi.fn(),
  }
  return render(<ChatInput {...defaultProps} {...props} />, { wrapper: I18nProvider })
}

/** The segmented control's group (jsdom has no matchMedia → the wide form). */
function modeGroup(): HTMLElement {
  return screen.getByLabelText('Permission mode')
}

// The shared setup stubs matchMedia with `matches` true ONLY for
// prefers-reduced-motion — the segmented control's breakpoint query would
// read false and render the <1200px chip fallback. This suite pins the
// SEGMENTED form, so width queries resolve against the wide (1440px) e2e
// viewport profile here.
const realMatchMedia = window.matchMedia
beforeAll(() => {
  window.matchMedia = vi.fn().mockImplementation((query: string) => ({
    matches:
      /min-width:\s*1200px/.test(query) ||
      /prefers-reduced-motion:\s*reduce/i.test(query),
    media: query,
    onchange: null,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    addListener: vi.fn(),
    removeListener: vi.fn(),
    dispatchEvent: vi.fn(),
  })) as unknown as typeof window.matchMedia
})
afterAll(() => {
  window.matchMedia = realMatchMedia
})

function radio(value: string): HTMLElement {
  return within(modeGroup()).getByTestId(`approval-mode-segment-${value}`)
}

/** The one segment whose aria-checked is true ("" when none). */
function checkedValue(): string {
  for (const el of within(modeGroup()).getAllByRole('radio')) {
    if (el.getAttribute('aria-checked') === 'true') {
      return el.getAttribute('data-testid')?.replace('approval-mode-segment-', '') ?? ''
    }
  }
  return ''
}

describe('ChatInput approval-mode segmented control (Aurora 2026-10, 裁决 B1)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    configOverride = makeConfig('ask')
    vi.mocked(api.configure).mockReset()
    vi.mocked(api.configure).mockResolvedValue(undefined)
  })

  it('renders the four ladder stops — ask / auto-edit / plan / full-auto', () => {
    renderChatInput()
    expect(within(modeGroup()).getAllByRole('radio')).toHaveLength(4)
    expect(radio('ask')).toBeInTheDocument()
    expect(radio('auto-edit')).toBeInTheDocument()
    // Plan takes its design-mandated seat: the engine accepts `plan` as an
    // approval_mode (ApprovalMode::Plan).
    expect(radio('plan')).toBeInTheDocument()
    expect(radio('full-auto')).toBeInTheDocument()
  })

  it('renders the SHARED table label for the current config mode (auto_edit → Auto Edit checked)', () => {
    configOverride = makeConfig('auto_edit')
    renderChatInput()
    // Legacy 'auto_edit' normalizes to the auto-edit ladder tier — its
    // segment is the checked one, proving the composer reads the same table
    // the settings page writes.
    expect(checkedValue()).toBe('auto-edit')
    expect(radio('auto-edit')).toHaveTextContent(/auto edit/i)
  })

  it('suggest renders as the Ask tier (legacy alias of the ladder base)', () => {
    renderChatInput()
    expect(checkedValue()).toBe('ask')
  })

  it('renders unknown engine values honestly via the raw badge — no segment claims them', () => {
    configOverride = makeConfig('mystery-mode')
    renderChatInput()
    // A value this UI does not manage shows raw in the badge instead of
    // dressing itself up as a tier the user could meaningfully pick.
    expect(within(modeGroup()).getByTestId('approval-mode-offladder-badge')).toHaveTextContent('mystery-mode')
    expect(checkedValue()).toBe('')
  })

  it('legacy confirm normalizes to the Ask tier (no fake raw readout)', () => {
    configOverride = makeConfig('confirm')
    renderChatInput()
    expect(checkedValue()).toBe('ask')
  })

  it('picking a segment commits configure(approval_mode) + refreshConfig — the General-page write', async () => {
    renderChatInput()
    fireEvent.click(radio('auto-edit'))
    // The commit chain is async (click → await configure → await
    // refreshConfig). Order-specific: refreshConfig is only asserted after
    // configure landed.
    await waitFor(() => expect(api.configure).toHaveBeenCalledWith({ key: 'approval_mode', value: 'auto-edit' }), { timeout: 15_000 })
    await waitFor(() => expect(mockRefreshConfig).toHaveBeenCalled(), { timeout: 15_000 })
  })

  it('the plan segment writes the engine-legal plan value (and replaces the old toggle)', async () => {
    renderChatInput()
    fireEvent.click(radio('plan'))
    await waitFor(() => expect(api.configure).toHaveBeenCalledWith({ key: 'approval_mode', value: 'plan' }), { timeout: 15_000 })
    // The standalone plan-mode toggle is gone — one surface owns the key.
    expect(screen.queryByRole('button', { name: 'Toggle plan mode' })).not.toBeInTheDocument()
  })

  it('the High-risk note travels with the switcher (group title)', () => {
    renderChatInput()
    expect(modeGroup()).toHaveAttribute('title', expect.stringContaining('High-risk actions'))
  })

  it('an out-of-table current value never hides the listed tiers (plan_ro → Strict badge)', () => {
    configOverride = makeConfig('plan_ro')
    renderChatInput()
    const group = modeGroup()
    // Legacy plan_ro normalizes to the readonly EXPERT mode — labeled
    // honestly (Strict) via the badge, while all four stops stay pickable.
    expect(within(group).getByTestId('approval-mode-offladder-badge')).toHaveTextContent(/strict/i)
    expect(within(group).getAllByRole('radio')).toHaveLength(4)
    expect(checkedValue()).toBe('')
    // The honest description + the high-risk note still ride the title.
    expect(group).toHaveAttribute('title', expect.stringContaining('High-risk actions'))
  })
})
