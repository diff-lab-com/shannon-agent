// GB P2-4 — composer approval-tier switcher tests (round-1 R3: four tiers).
//
// Pins the composer↔settings sync contract: the pill renders the SHARED
// table's label for the current `config.approval_mode`, switching commits
// the same configure('approval_mode') write the General page performs, the
// High-risk note travels with the menu, and `confirm` reads out as the raw
// engine value instead of pretending to be a pickable tier.

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import ChatInput from '@/components/chat/ChatInput'
import * as api from '@/lib/tauri-api'
import type * as ReactRouterDom from 'react-router-dom'

// 300s, not the 120s this file carried since GB round-1: the commit-chain
// test below has three bounded waits (openSelectOptions 15s + two waitFor
// 15s) whose worst case (~45s + render overhead) already forced a 60s → 120s
// bump when the 2-core CI runners contended (2026-10-01). Under the 4-worker
// unit gate (#242 follow-up) the same test hit the 120s ceiling once more
// (run 37117293111, shard 2/2) — heavy vitest fork contention again, healthy
// path still ~45s. 300s = headroom for the noisy-neighbor regime; a genuinely
// broken interaction still fails fast at its own 15s bounds.
vi.setConfig({ testTimeout: 300_000 })

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

// Round-3 CI hardening: the popup portal normally mounts synchronously
// under fireEvent's act (and the pick below deliberately stays synchronous —
// that timing is what the suite has always exercised). But on a saturated
// single-thread CI runner the mount can lag; when the sync read finds no
// popup yet, these bounded retries poll instead of failing. Same
// determinism, no skip.
const POPUP_TIMEOUT = 15_000

const sleep = (ms: number) => new Promise(r => setTimeout(r, ms))

/**
 * Sync-first options read: returns the options when the popup is already
 * mounted (the proven timing), otherwise re-clicks the trigger and polls —
 * re-clicks only ever happen while the portal is ABSENT, so a mounted
 * popup is never toggled closed.
 */
async function openSelectOptions(trigger: HTMLElement, count: number): Promise<HTMLElement[]> {
  let opts = currentOptions()
  if (opts.length >= count) return opts
  const deadline = Date.now() + POPUP_TIMEOUT
  for (;;) {
    if (!document.querySelector('[data-slot="select-content"]')) fireEvent.click(trigger)
    opts = currentOptions()
    if (opts.length >= count) return opts
    if (Date.now() + 80 > deadline) {
      throw new Error(`select popup: expected >=${count} options, saw ${opts.length}`)
    }
    await sleep(60)
  }
}

describe('ChatInput approval-mode switcher (GB P2-4, round-1 R3)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    configOverride = makeConfig('suggest')
    vi.mocked(api.configure).mockReset()
    vi.mocked(api.configure).mockResolvedValue(undefined)
  })

  afterEach(() => {
    document.querySelectorAll('[data-slot="select-content"]').forEach(n => n.remove())
  })

  it('renders the SHARED table label for the current config mode (auto_edit → Permissive)', () => {
    configOverride = makeConfig('auto_edit')
    renderChatInput()
    const pill = screen.getByLabelText('Permission mode')
    // 'auto_edit' is the permissive tier — the pill shows ITS label, proving
    // the composer reads the same table the settings page writes.
    expect(pill).toHaveTextContent(/permissive/i)
  })

  it('suggest renders as the Balanced tier (R3 naming by engine semantics)', () => {
    renderChatInput()
    expect(screen.getByLabelText('Permission mode')).toHaveTextContent(/balanced/i)
  })

  it('renders out-of-table engine values honestly via the raw string (R3: confirm)', () => {
    configOverride = makeConfig('confirm')
    renderChatInput()
    // confirm ≡ suggest engine-side; the pill shows the raw value instead of
    // dressing it up as a tier the user could meaningfully pick.
    expect(screen.getByLabelText('Permission mode')).toHaveTextContent('confirm')
  })

  it('picking a tier commits configure(approval_mode) + refreshConfig — the General-page write', async () => {
    renderChatInput()
    const trigger = screen.getByLabelText('Permission mode')
    fireEvent.click(trigger)
    const opts = await openSelectOptions(trigger, 4)
    const permissive = opts.find(o => /permissive/i.test(o.textContent ?? ''))
    expect(permissive).toBeTruthy()
    fireEvent.pointerDown(permissive!, { button: 0 })
    fireEvent.pointerUp(permissive!, { button: 0 })
    fireEvent.click(permissive!)
    // The commit chain is async (Base UI commit → onValueChange → await
    // configure → await refreshConfig) — generous explicit timeouts replace
    // waitFor's 1s default that the round-3 CI runner outran. Order-specific:
    // refreshConfig is only asserted after configure landed.
    await waitFor(() => expect(api.configure).toHaveBeenCalledWith({ key: 'approval_mode', value: 'auto_edit' }), { timeout: POPUP_TIMEOUT })
    await waitFor(() => expect(mockRefreshConfig).toHaveBeenCalled(), { timeout: POPUP_TIMEOUT })
  })

  it('the High-risk note travels with the switcher menu', async () => {
    renderChatInput()
    const trigger = screen.getByLabelText('Permission mode')
    fireEvent.click(trigger)
    await openSelectOptions(trigger, 4)
    const popups = document.querySelectorAll('[data-slot="select-content"]')
    const last = popups[popups.length - 1]
    expect(last?.textContent).toMatch(/high-risk actions/i)
    expect(last?.textContent).toMatch(/baseline/i)
  })

  it('an out-of-table current value never hides the listed tiers (pill is display-only)', () => {
    configOverride = makeConfig('dont_ask')
    renderChatInput()
    const pill = screen.getByLabelText('Permission mode')
    expect(pill).toHaveTextContent('dont_ask')
    // The title still carries the honest description + the high-risk note.
    expect(pill).toHaveAttribute('title', expect.stringContaining('High-risk actions'))
  })
})
