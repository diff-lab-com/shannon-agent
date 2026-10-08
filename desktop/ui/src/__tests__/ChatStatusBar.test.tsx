// ChatStatusBar — Aurora redesign 2026-10 (02-chat.html 状态条).
//
// Pins the HONESTY CONTRACT: each segment renders only when its data source
// actually reports something — context % requires the engine-resolved
// `context_total` on the usage payload, the cost segment requires a
// non-zero session ledger entry, the budget segment only exists while a
// positive cap is configured. The working-directory segment doubles as the
// WD-picker entry (the old ComposerPanel footer row moved into the bar:
// same aria label, breadcrumb and disabled-without-session behavior).

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { I18nProvider } from '@/i18n'

import ChatStatusBar from '@/components/chat/ChatStatusBar'
import { clearDiffStatsCache } from '@/components/chat/diffStats'
import type { SessionBudgetState } from '@/hooks/useSessionBudget'
import type { UsagePayload } from '@/types'

const budgetState = vi.hoisted(() => ({
  current: {
    budget: null as number | null,
    usage: null as any,
    refresh: () => {},
  } as SessionBudgetState,
}))

vi.mock('@/hooks/useSessionBudget', () => ({
  useSessionBudget: () => budgetState.current,
}))

// Branch segment IPC — defaults to "not a git repo" (null → hidden); the
// branch tests below override the resolved value per case.
const tauriApi = vi.hoisted(() => ({ currentGitBranch: vi.fn() }))
vi.mock('@/lib/tauri-api', () => ({ currentGitBranch: tauriApi.currentGitBranch }))

function renderBar(usage: UsagePayload | null, workingDir = '/home/alice/code/myproject', sessionId: string | null = 'session-1') {
  return render(
    <I18nProvider>
      <ChatStatusBar
        workingDir={workingDir}
        usage={usage}
        sessionId={sessionId}
        onChangeWorkingDir={() => {}}
      />
    </I18nProvider>,
  )
}

const baseUsage: UsagePayload = {
  query_id: 'q1',
  input_tokens: 38_000,
  output_tokens: 36_000,
  cost_usd: 0.0872,
  context_total: 200_000,
}

beforeEach(() => {
  clearDiffStatsCache()
  budgetState.current = { budget: null, usage: null, refresh: () => {} }
  tauriApi.currentGitBranch.mockResolvedValue(null)
})

describe('ChatStatusBar — working directory segment', () => {
  it('renders the breadcrumb and keeps the WD-picker entry point', () => {
    renderBar(null)
    const button = screen.getByRole('button', { name: 'Change working directory for this chat' })
    expect(button).toBeEnabled()
    expect(screen.getByText('…/code/myproject')).toBeInTheDocument()
  })

  it('shows "Not set" and disables the picker without a session', () => {
    renderBar(null, '', null)
    expect(screen.getByRole('button', { name: 'Change working directory for this chat' })).toBeDisabled()
    expect(screen.getByText('Not set')).toBeInTheDocument()
  })
})

describe('ChatStatusBar — honesty contract', () => {
  it('renders context %, session cost and budget left when their sources report data', () => {
    budgetState.current = {
      budget: 20,
      usage: { input_tokens: 1, output_tokens: 1, cache_creation_tokens: 0, cache_read_tokens: 0, cost_usd: 0.0872, events: 3 },
      refresh: () => {},
    }
    renderBar(baseUsage)
    // Aurora 2026-10: the segment carries the raw token counts too
    // (设计稿 02: 上下文 38% · 74k/200k) — compact locale formatting.
    expect(screen.getByTestId('chat-status-context')).toHaveTextContent('Context 37% · 74K/200K')
    expect(screen.getByTestId('chat-status-cost')).toHaveTextContent('$0.0872')
    expect(screen.getByTestId('chat-status-budget-left')).toHaveTextContent('$19.91')
  })

  it('hides the context segment when the usage payload has no resolved window', () => {
    budgetState.current = {
      budget: null,
      usage: { input_tokens: 1, output_tokens: 1, cache_creation_tokens: 0, cache_read_tokens: 0, cost_usd: 0.5, events: 1 },
      refresh: () => {},
    }
    // 批 1: the denominator is the engine-resolved `context_total` — absent
    // (unknown window) → no percentage, never a fabricated one.
    renderBar({ ...baseUsage, context_total: undefined })
    expect(screen.queryByTestId('chat-status-context')).toBeNull()
    // The cost segment still renders.
    expect(screen.getByTestId('chat-status-cost')).toBeInTheDocument()
  })

  it('hides the context segment when the frame reports no tokens yet', () => {
    // A resolved window alone is not enough: 0/0 tokens → nothing to show.
    renderBar({ ...baseUsage, input_tokens: 0, output_tokens: 0 })
    expect(screen.queryByTestId('chat-status-context')).toBeNull()
  })

  it('clamps the context percentage at 100 when tokens exceed the window', () => {
    renderBar({ ...baseUsage, input_tokens: 190_000, output_tokens: 190_000 })
    const seg = screen.getByTestId('chat-status-context')
    expect(seg.textContent).toContain('100%')
  })

  it('hides the cost segment while the session ledger has not observed spend', () => {
    budgetState.current = {
      budget: null,
      usage: { input_tokens: 0, output_tokens: 0, cache_creation_tokens: 0, cache_read_tokens: 0, cost_usd: 0, events: 0 },
      refresh: () => {},
    }
    renderBar(null)
    expect(screen.queryByTestId('chat-status-cost')).toBeNull()
  })

  it('hides the budget segment without a configured cap and flags an exhausted one', () => {
    budgetState.current = {
      budget: null,
      usage: { input_tokens: 1, output_tokens: 1, cache_creation_tokens: 0, cache_read_tokens: 0, cost_usd: 2, events: 1 },
      refresh: () => {},
    }
    renderBar(null)
    expect(screen.queryByTestId('chat-status-budget-left')).toBeNull()

    budgetState.current = {
      budget: 1,
      usage: { input_tokens: 1, output_tokens: 1, cache_creation_tokens: 0, cache_read_tokens: 0, cost_usd: 1.5, events: 1 },
      refresh: () => {},
    }
    renderBar(null)
    const left = screen.getByTestId('chat-status-budget-left')
    expect(left).toHaveTextContent('$0.00')
    expect(left.className).toContain('text-error')
  })
})

describe('ChatStatusBar — branch + cache segments (2026-10-08 round)', () => {
  it('renders the working dir branch once the IPC resolves', async () => {
    tauriApi.currentGitBranch.mockResolvedValue('fix/billing-webhook')
    renderBar(null)
    const seg = await screen.findByTestId('chat-status-branch')
    expect(seg).toHaveTextContent('fix/billing-webhook')
    expect(tauriApi.currentGitBranch).toHaveBeenCalledWith('/home/alice/code/myproject')
  })

  it('hides the branch segment when the dir is not a git repo', async () => {
    renderBar(null)
    await waitFor(() => expect(tauriApi.currentGitBranch).toHaveBeenCalled())
    expect(screen.queryByTestId('chat-status-branch')).not.toBeInTheDocument()
  })

  it('renders the cache-hit segment from the usage frame', () => {
    renderBar({ ...baseUsage, cache_hit_rate: 0.92 })
    const seg = screen.getByTestId('chat-status-cache')
    expect(seg).toHaveTextContent('92%')
  })

  it('hides the cache segment while the frame carries no rate', () => {
    renderBar(baseUsage)
    expect(screen.queryByTestId('chat-status-cache')).not.toBeInTheDocument()
  })
})
