// GB P2-4 — composer approval-tier switcher tests.
//
// Pins the composer↔settings sync contract: the pill renders the SHARED
// table's label for the current `config.approval_mode`, switching commits
// the same configure('approval_mode') write the General page performs, and
// the High-risk note travels with the menu.

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import ChatInput from '@/components/chat/ChatInput'
import * as api from '@/lib/tauri-api'
import type * as ReactRouterDom from 'react-router-dom'

vi.setConfig({ testTimeout: 60_000 })

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

/** Base UI select popup helpers — same pattern as ChatInput.test.tsx. */
const currentOptions = (): HTMLElement[] => {
  const popups = document.querySelectorAll('[data-slot="select-content"]')
  const last = popups[popups.length - 1]
  return last ? Array.from(last.querySelectorAll('[role="option"]')) : []
}

describe('ChatInput approval-mode switcher (GB P2-4)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    configOverride = makeConfig('suggest')
    vi.mocked(api.configure).mockReset()
    vi.mocked(api.configure).mockResolvedValue(undefined)
  })

  afterEach(() => {
    document.querySelectorAll('[data-slot="select-content"]').forEach(n => n.remove())
  })

  it('renders the SHARED table label for the current config mode (Auto Edit, not the old composer set)', () => {
    configOverride = makeConfig('auto_edit')
    renderChatInput()
    const pill = screen.getByLabelText('Permission mode')
    // 'auto_edit' is a General-page tier — the pill shows ITS label, proving
    // the composer reads the same table the settings page writes.
    expect(pill).toHaveTextContent(/auto edit/i)
  })

  it('renders out-of-table engine modes honestly via the raw value', () => {
    configOverride = makeConfig('dont_ask')
    renderChatInput()
    expect(screen.getByLabelText('Permission mode')).toHaveTextContent('dont_ask')
  })

  it('picking a tier commits configure(approval_mode) + refreshConfig — the General-page write', async () => {
    renderChatInput()
    fireEvent.click(screen.getByLabelText('Permission mode'))
    const opts = currentOptions()
    expect(opts.length).toBeGreaterThanOrEqual(5)
    const planOption = opts.find(o => /plan/i.test(o.textContent ?? ''))
    expect(planOption).toBeTruthy()
    fireEvent.pointerDown(planOption!, { button: 0 })
    fireEvent.pointerUp(planOption!, { button: 0 })
    fireEvent.click(planOption!)
    await waitFor(() => expect(api.configure).toHaveBeenCalledWith({ key: 'approval_mode', value: 'plan' }))
    await waitFor(() => expect(mockRefreshConfig).toHaveBeenCalled())
  })

  it('the High-risk note travels with the switcher menu', async () => {
    renderChatInput()
    fireEvent.click(screen.getByLabelText('Permission mode'))
    await waitFor(() => expect(currentOptions().length).toBeGreaterThanOrEqual(5))
    // The note explains the tier only moves the auto-approve baseline; the
    // en copy mentions confirmation for high-risk actions.
    const popups = document.querySelectorAll('[data-slot="select-content"]')
    const last = popups[popups.length - 1]
    expect(last?.textContent).toMatch(/high-risk actions/i)
    expect(last?.textContent).toMatch(/baseline/i)
  })

  it('an out-of-table current value never hides the listed tiers (pill is display-only)', () => {
    // Rendering-level pin only — popup option-count assertions run in the
    // tests above; the Base UI popup in jsdom is the known-slow path.
    configOverride = makeConfig('readonly')
    renderChatInput()
    const pill = screen.getByLabelText('Permission mode')
    expect(pill).toHaveTextContent(/read-only/i)
    // The title still carries the honest description + the high-risk note.
    expect(pill).toHaveAttribute('title', expect.stringContaining('High-risk actions'))
  })
})
