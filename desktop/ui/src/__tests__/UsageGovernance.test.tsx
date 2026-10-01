import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, screen, waitFor, fireEvent } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import { MemoryRouter } from 'react-router-dom'
import Usage from '@/pages/Usage'
import { AppProvider } from '@/context/AppContext'
import * as api from '@/lib/tauri-api'
import type { UsageGovernance, UsageStats } from '@/types'

// Empty usage ledger so the page renders its static surfaces.
const emptyStats: UsageStats = {
  days: 30,
  totals: { label: 'total', input_tokens: 0, output_tokens: 0, cache_creation_tokens: 0, cache_read_tokens: 0, cost_usd: 0, requests: 0 },
  by_model: [],
  by_provider: [],
  by_day: [],
}

function governance(partial: Partial<UsageGovernance>): UsageGovernance {
  return {
    month: '2026-10',
    monthCostUsd: 4.0,
    last7dCostUsd: 1.5,
    budgetUsd: 10.0,
    percent: 40,
    warned80: false,
    hit100: false,
    thresholdReached: null,
    ...partial,
  }
}

function renderUsage() {
  return render(
    <I18nProvider>
      <MemoryRouter>
        <AppProvider>
          <Usage />
        </AppProvider>
      </MemoryRouter>
    </I18nProvider>,
  )
}

describe('Usage governance (P2-1)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(api.getUsageStats).mockReset()
    vi.mocked(api.getUsageStats).mockResolvedValue(emptyStats)
    vi.mocked(api.getUsageGovernance).mockReset()
    vi.mocked(api.getUsageGovernance).mockResolvedValue(null)
  })

  it('renders the budget card with bar and percent when a budget is set', async () => {
    vi.mocked(api.getUsageGovernance).mockResolvedValue(governance({}))
    renderUsage()

    await waitFor(() => {
      expect(screen.getByTestId('usage-budget-card')).toBeInTheDocument()
    })
    expect(screen.getByTestId('usage-budget-fill')).toHaveStyle({ width: '40%' })
    expect(screen.getByTestId('usage-budget-percent')).toHaveTextContent('40%')
    // The budget input carries the persisted value.
    expect(screen.getByTestId('usage-budget-input')).toHaveValue(10)
  })

  it('shows the 80% warning banner (amber) at the warn threshold', async () => {
    vi.mocked(api.getUsageGovernance).mockResolvedValue(
      governance({ percent: 85, monthCostUsd: 8.5, thresholdReached: '80' }),
    )
    renderUsage()

    await waitFor(() => {
      expect(screen.getByTestId('usage-budget-banner')).toBeInTheDocument()
    })
    expect(screen.getByTestId('usage-budget-banner')).toHaveTextContent('85% of your monthly budget')
    expect(screen.getByTestId('usage-budget-banner')).toHaveTextContent('$8.50')
  })

  it('shows the 100% banner (error) at/over the cap', async () => {
    vi.mocked(api.getUsageGovernance).mockResolvedValue(
      governance({ percent: 120, monthCostUsd: 12, thresholdReached: '100' }),
    )
    renderUsage()

    await waitFor(() => {
      expect(screen.getByTestId('usage-budget-banner')).toHaveTextContent('Monthly budget reached')
    })
  })

  it('hides banner and card without a budget', async () => {
    vi.mocked(api.getUsageGovernance).mockResolvedValue(governance({ budgetUsd: null, percent: null }))
    renderUsage()

    await waitFor(() => {
      expect(screen.queryByTestId('usage-budget-card')).not.toBeInTheDocument()
    })
    expect(screen.queryByTestId('usage-budget-banner')).not.toBeInTheDocument()
  })

  it('persists the budget via configure(monthly_budget_usd) and refreshes', async () => {
    vi.mocked(api.getUsageGovernance).mockResolvedValue(governance({}))
    renderUsage()

    await waitFor(() => {
      expect(screen.getByTestId('usage-budget-input')).toBeInTheDocument()
    })
    const input = screen.getByTestId('usage-budget-input')
    fireEvent.change(input, { target: { value: '25' } })
    fireEvent.click(screen.getByRole('button', { name: 'Save budget' }))

    await waitFor(() => {
      expect(api.configure).toHaveBeenCalledWith({ key: 'monthly_budget_usd', value: '25' })
    })
    // The card re-pulls governance after a save (fresh % bar).
    await waitFor(() => {
      expect(api.getUsageGovernance.mock.calls.length).toBeGreaterThanOrEqual(2)
    })
  })
})
