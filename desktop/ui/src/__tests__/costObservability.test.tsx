// Tests for the P0-4 cost-observability frontend:
//   - ContextBreakdownCard: six-category rendering, share math, window %,
//     cache hit rate (incl. the zero-denominator "no data" case), session cost
//   - BudgetBanner: warning vs exceeded bars + the three exceeded actions
//   - BudgetDialog: save/clear dispatch + invalid input
//   - Billing demo removal (D5): no API surface, no tab label

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import ContextBreakdownCard from '@/components/chat/ContextBreakdownCard'
import BudgetBanner from '@/components/chat/BudgetBanner'
import BudgetDialog from '@/components/chat/BudgetDialog'
import * as api from '@/lib/tauri-api'
import type { ContextBreakdown } from '@/types'

vi.mock('@/lib/tauri-api', async () => {
  const actual = await vi.importActual<object>('@/lib/tauri-api')
  return {
    ...actual,
    getSessionContextBreakdown: vi.fn(),
    getSessionUsage: vi.fn(),
    setSessionBudget: vi.fn(),
  }
})

vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}))

const wrapper = ({ children }: { children: React.ReactNode }) => (
  <I18nProvider>{children}</I18nProvider>
)

const mockedBreakdown = vi.mocked(api.getSessionContextBreakdown)
const mockedSessionUsage = vi.mocked(api.getSessionUsage)

function makeBreakdown(over: Partial<ContextBreakdown> = {}): ContextBreakdown {
  return {
    totalTokens: 1000,
    contextWindow: 200_000,
    categories: [
      { key: 'system', tokens: 200 },
      { key: 'tools', tokens: 300 },
      { key: 'skills', tokens: 100 },
      { key: 'memory', tokens: 50 },
      { key: 'mcp', tokens: 0 },
      { key: 'conversation', tokens: 350 },
    ],
    ...over,
  }
}

beforeEach(() => {
  vi.clearAllMocks()
})

// ── ContextBreakdownCard ────────────────────────────────────────────────

describe('ContextBreakdownCard', () => {
  it('renders all six categories with tokens and share percentages', async () => {
    mockedBreakdown.mockResolvedValue(makeBreakdown())
    mockedSessionUsage.mockResolvedValue({
      input_tokens: 100, output_tokens: 50, cache_creation_tokens: 0, cache_read_tokens: 0, cost_usd: 0.05, events: 2,
    })

    render(<ContextBreakdownCard sessionId="s1" usageTick={null} />, { wrapper })

    // Total = 1000 → system 200 = 20%, conversation 350 = 35%.
    await waitFor(() => expect(screen.getByText('System prompt')).toBeTruthy())
    expect(screen.getByText('Conversation')).toBeTruthy()
    expect(screen.getByText('20%')).toBeTruthy()
    expect(screen.getByText('35%')).toBeTruthy()
    // MCP is empty → "—" share, but the row still renders.
    expect(screen.getByText('MCP tools')).toBeTruthy()
    // Per-category aria labels carry tokens + share (keyboard reachable rows).
    expect(screen.getByLabelText('System prompt: 200 (20%)')).toBeTruthy()
    expect(screen.getByLabelText('Conversation: 350 (35%)')).toBeTruthy()
    expect(screen.getByLabelText('MCP tools: 0 (—)')).toBeTruthy()
    // Window occupancy: 1000 / 200000 = 0.5% (the compact number format
    // follows the provider locale, so only the percentage is asserted).
    expect(screen.getByText(/0\.5% of the .* window/)).toBeTruthy()
  })

  it('computes the cache hit rate from the cumulative session usage', async () => {
    mockedBreakdown.mockResolvedValue(makeBreakdown())
    // cache_read / (cache_read + input) = 300 / (300 + 100) = 75%.
    mockedSessionUsage.mockResolvedValue({
      input_tokens: 100, output_tokens: 50, cache_creation_tokens: 0, cache_read_tokens: 300, cost_usd: 1.25, events: 4,
    })

    render(<ContextBreakdownCard sessionId="s1" usageTick={null} />, { wrapper })

    await waitFor(() => expect(screen.getByText('75.0%')).toBeTruthy())
    // Session cumulative cost comes from the same summary.
    expect(screen.getByText('$1.2500')).toBeTruthy()
  })

  it('shows "No data yet" for a zero cache denominator', async () => {
    mockedBreakdown.mockResolvedValue(makeBreakdown())
    mockedSessionUsage.mockResolvedValue({
      input_tokens: 0, output_tokens: 0, cache_creation_tokens: 0, cache_read_tokens: 0, cost_usd: 0, events: 0,
    })

    render(<ContextBreakdownCard sessionId="s1" usageTick={null} />, { wrapper })

    await waitFor(() => expect(screen.getByText('No data yet')).toBeTruthy())
    expect(screen.queryByText('%')).not.toBeTruthy()
  })
})

// ── BudgetBanner ────────────────────────────────────────────────────────

describe('BudgetBanner', () => {
  const payload = { sessionId: 's1', spentUsd: 5.2, budgetUsd: 5 }

  it('renders the yellow warning bar with spent/budget and dismisses', () => {
    const clearWarning = vi.fn()
    render(
      <BudgetBanner warning={payload} exceeded={null} clearWarning={clearWarning}
        clearExceeded={vi.fn()} onContinueOnce={vi.fn()} sessionId="s1" />,
      { wrapper },
    )
    expect(screen.getByText('Approaching the budget limit')).toBeTruthy()
    expect(screen.getByText(/5\.20 of \$5\.00/)).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }))
    expect(clearWarning).toHaveBeenCalledTimes(1)
  })

  it('offers the three exceeded actions and dispatches them', () => {
    const onContinueOnce = vi.fn()
    const clearExceeded = vi.fn()
    render(
      <BudgetBanner warning={null} exceeded={payload} clearWarning={vi.fn()}
        clearExceeded={clearExceeded} onContinueOnce={onContinueOnce} sessionId="s1" />,
      { wrapper },
    )
    expect(screen.getByText('Session budget exceeded')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: 'Continue (ignore once)' }))
    expect(onContinueOnce).toHaveBeenCalledTimes(1)
    expect(clearExceeded).toHaveBeenCalledTimes(1)

    // "Stop" only clears the banner.
    fireEvent.click(screen.getByRole('button', { name: 'Stop' }))
    expect(clearExceeded).toHaveBeenCalledTimes(2)

    // "Raise budget…" opens the dialog pre-filled with the current cap.
    fireEvent.click(screen.getByRole('button', { name: 'Raise budget…' }))
    expect(screen.getByText('Set session budget')).toBeTruthy()
    expect((screen.getByPlaceholderText('e.g. 5.00') as HTMLInputElement).value).toBe('5')
  })
})

// ── BudgetDialog ────────────────────────────────────────────────────────

describe('BudgetDialog', () => {
  it('saves a valid amount through setSessionBudget', async () => {
    const onSaved = vi.fn()
    const onClose = vi.fn()
    mockedBreakdown.mockResolvedValue(makeBreakdown())
    vi.mocked(api.setSessionBudget).mockResolvedValue(undefined)

    render(<BudgetDialog open sessionId="s1" budget={null} onClose={onClose} onSaved={onSaved} />, { wrapper })
    const input = screen.getByPlaceholderText('e.g. 5.00')
    fireEvent.change(input, { target: { value: '7.5' } })
    fireEvent.click(screen.getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(api.setSessionBudget).toHaveBeenCalledWith('s1', 7.5))
    expect(onSaved).toHaveBeenCalledWith(7.5)
    expect(onClose).toHaveBeenCalled()
  })

  it('clears with null', async () => {
    const onSaved = vi.fn()
    vi.mocked(api.setSessionBudget).mockResolvedValue(undefined)

    render(<BudgetDialog open sessionId="s1" budget={5} onClose={vi.fn()} onSaved={onSaved} />, { wrapper })

    fireEvent.click(screen.getByRole('button', { name: 'Clear budget' }))
    await waitFor(() => expect(api.setSessionBudget).toHaveBeenCalledWith('s1', null))
    expect(onSaved).toHaveBeenCalledWith(null)
  })

  it('rejects non-positive input with an inline alert and no save', async () => {
    const onSaved = vi.fn()
    vi.mocked(api.setSessionBudget).mockResolvedValue(undefined)

    render(<BudgetDialog open sessionId="s1" budget={null} onClose={vi.fn()} onSaved={onSaved} />, { wrapper })
    const input = screen.getByPlaceholderText('e.g. 5.00')
    fireEvent.change(input, { target: { value: '-2' } })
    fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    await waitFor(() => expect(screen.getByRole('alert')).toBeTruthy())
    expect(api.setSessionBudget).not.toHaveBeenCalled()
  })
})

// ── Billing demo removal (decision D5) ──────────────────────────────────

describe('billing demo removal', () => {
  it('exposes no billing API surface anymore', async () => {
    const actual = (await vi.importActual('@/lib/tauri-api')) as Record<string, unknown>
    expect(actual.getBillingPlan).toBeUndefined()
    expect(actual.getCostHistory).toBeUndefined()
    expect(actual.getBillingHistory).toBeUndefined()
  })
})
