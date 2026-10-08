// ChatStatusBar — Aurora redesign 2026-10 (02-chat.html 状态条).
//
// Pins the HONESTY CONTRACT: each segment renders only when its data source
// actually reports something — context % requires `max_tokens` on the usage
// payload, the cost segment requires a non-zero session ledger entry, the
// budget segment only exists while a positive cap is configured. The
// working-directory segment doubles as the WD-picker entry (the old
// ComposerPanel footer row moved into the bar: same aria label, breadcrumb
// and disabled-without-session behavior).

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen } from '@testing-library/react'
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
  max_tokens: 200_000,
}

beforeEach(() => {
  clearDiffStatsCache()
  budgetState.current = { budget: null, usage: null, refresh: () => {} }
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

  it('hides the context segment when the usage payload has no max_tokens', () => {
    budgetState.current = {
      budget: null,
      usage: { input_tokens: 1, output_tokens: 1, cache_creation_tokens: 0, cache_read_tokens: 0, cost_usd: 0.5, events: 1 },
      refresh: () => {},
    }
    renderBar({ ...baseUsage, max_tokens: undefined })
    expect(screen.queryByTestId('chat-status-context')).toBeNull()
    // The cost segment still renders.
    expect(screen.getByTestId('chat-status-cost')).toBeInTheDocument()
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
