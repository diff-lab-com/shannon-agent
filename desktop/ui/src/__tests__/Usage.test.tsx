import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import { MemoryRouter } from 'react-router-dom'
import Usage from '@/pages/Usage'
import { AppProvider } from '@/context/AppContext'
import * as api from '@/lib/tauri-api'
import type { SessionUsageRow, UsageStats } from '@/types'

const bucket = {
  input_tokens: 1000,
  output_tokens: 500,
  cache_creation_tokens: 100,
  cache_read_tokens: 50,
  cost_usd: 0.25,
  requests: 3,
}

const fixture: UsageStats = {
  days: 30,
  totals: { label: 'total', ...bucket },
  by_model: [{ label: 'claude-sonnet-4-6', ...bucket }],
  by_provider: [{ label: 'anthropic', ...bucket }],
  by_day: [{ label: '2024-01-02', ...bucket }],
}

function renderUsage() {
  return render(
    <I18nProvider>
      <MemoryRouter>
        {/* CurrentSessionCostPanel consumes useSessions — provide the
            context (AppProvider hosts the SessionContext.Provider) so the
            page mounts standalone in tests. */}
        <AppProvider>
          <Usage />
        </AppProvider>
      </MemoryRouter>
    </I18nProvider>,
  )
}

describe('Usage page', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(api.getUsageStats).mockReset()
  })

  it('renders totals and charts when data is present (overview)', async () => {
    vi.mocked(api.getUsageStats).mockResolvedValue(fixture)
    renderUsage()

    await waitFor(() => {
      expect(screen.getByText('claude-sonnet-4-6')).toBeInTheDocument()
    })
    // Overview is the default surface — assert the chart titles, not the
    // audit-mode table headers.
    // F-4 (ui-redesign 06): the byDay chart title follows the selected
    // range (default 30) — "Daily tokens · 30 days".
    expect(screen.getByText('Daily tokens · 30 days')).toBeInTheDocument()
    expect(screen.getByText('Tokens by provider')).toBeInTheDocument()
    // 2024-01-02 still appears in the chart's x-axis labels (MM-DD slice).
    expect(screen.getByText('01-02')).toBeInTheDocument()
  })

  it('switches to audit tables when the toggle is pressed', async () => {
    vi.mocked(api.getUsageStats).mockResolvedValue(fixture)
    renderUsage()

    await waitFor(() => {
      expect(screen.getByText('claude-sonnet-4-6')).toBeInTheDocument()
    })
    // B6-37: the mode toggle is an aria-pressed button pair (it never
    // implemented the tab keyboard pattern).
    fireEvent.click(screen.getByRole('button', { name: /Audit \(table\)/ }))
    await waitFor(() => {
      expect(screen.getByText('By model')).toBeInTheDocument()
      expect(screen.getByText('By provider')).toBeInTheDocument()
      expect(screen.getByText('By day')).toBeInTheDocument()
      expect(screen.getByText('anthropic')).toBeInTheDocument()
      expect(screen.getByText('2024-01-02')).toBeInTheDocument()
    })
  })

  it('shows the empty state when nothing is recorded yet', async () => {
    vi.mocked(api.getUsageStats).mockResolvedValue({
      days: 30,
      totals: { label: 'total', ...bucket, requests: 0 },
      by_model: [],
      by_provider: [],
      by_day: [],
    })
    renderUsage()

    await waitFor(() => {
      expect(
        screen.getByText('No usage recorded yet. Send a message to start tracking.'),
      ).toBeInTheDocument()
    })
  })

  it('re-fetches when the day range changes', async () => {
    vi.mocked(api.getUsageStats).mockResolvedValue(fixture)
    renderUsage()

    await waitFor(() => expect(api.getUsageStats).toHaveBeenCalledWith(30))

    fireEvent.click(screen.getByText('7 days'))
    await waitFor(() => expect(api.getUsageStats).toHaveBeenCalledWith(7))
  })

  // F-4 (ui-redesign 06): the byDay chart title must match the active
  // range filter — switching 30 → 7 updates the title text, not just the
  // data behind it.
  it('updates the byDay chart title when the range filter changes', async () => {
    vi.mocked(api.getUsageStats).mockResolvedValue(fixture)
    renderUsage()

    await waitFor(() => {
      expect(screen.getByText('Daily tokens · 30 days')).toBeInTheDocument()
    })

    fireEvent.click(screen.getByText('7 days'))
    await waitFor(() => {
      expect(screen.getByText('Daily tokens · 7 days')).toBeInTheDocument()
    })
    expect(screen.queryByText('Daily tokens · 30 days')).not.toBeInTheDocument()
  })

  // 设计 06 (审查 R1 §06): the derived KPI pair — today's spend from the
  // local-day bucket (fixture has none for today → $0.00 is the honest
  // reading) and the cache hit rate from the frozen formula
  // cache_read / (cache_read + input) = 50/1050 ≈ 4.8%.
  it('renders the derived Today-spend and Cache-hit-rate KPI cards', async () => {
    vi.mocked(api.getUsageStats).mockResolvedValue(fixture)
    renderUsage()

    await waitFor(() => expect(screen.getByTestId('usage-stat-cache-hit-rate')).toBeInTheDocument())
    expect(screen.getByTestId('usage-stat-cache-hit-rate')).toHaveTextContent('4.8%')
    expect(screen.getByTestId('usage-stat-today-cost')).toHaveTextContent('$0.00')
  })

  // 设计 06: the daily chart carries the Token/成本 metric toggle — the
  // cost metric renames the card to its own title so title = data.
  it('switches the daily chart between the Token and cost metric', async () => {
    vi.mocked(api.getUsageStats).mockResolvedValue(fixture)
    renderUsage()

    await waitFor(() => expect(screen.getByText('Daily tokens · 30 days')).toBeInTheDocument())
    fireEvent.click(screen.getByRole('button', { name: 'Cost' }))
    await waitFor(() => expect(screen.getByText('Daily cost · 30 days')).toBeInTheDocument())
    expect(screen.queryByText('Daily tokens · 30 days')).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Tokens' }))
    await waitFor(() => expect(screen.getByText('Daily tokens · 30 days')).toBeInTheDocument())
  })

  // 设计 06: 按会话明细回到总览 — the overview previews the top 8 session
  // rows and honestly truncates with a pointer to the Audit view.
  it('previews the top 8 sessions in the overview with the audit hint', async () => {
    vi.mocked(api.getUsageStats).mockResolvedValue(fixture)
    const rows: SessionUsageRow[] = Array.from({ length: 11 }, (_, i) => ({
      sessionId: `sess-${i}`,
      title: `Session ${i}`,
      inputTokens: 10,
      outputTokens: 5,
      cacheCreationTokens: 0,
      cacheReadTokens: 0,
      costUsd: 0.1 * i,
      requests: 1,
      lastUsedAtMs: 1_700_000_000_000,
    }))
    vi.mocked(api.getUsageBySession).mockResolvedValue(rows)
    renderUsage()

    const preview = await screen.findByTestId('usage-session-preview')
    for (let i = 0; i < 8; i++) {
      expect(preview).toHaveTextContent(`Session ${i}`)
    }
    expect(preview).not.toHaveTextContent('Session 8')
    expect(screen.getByTestId('usage-session-preview-hint')).toHaveTextContent(
      'Showing first 8 sessions',
    )
  })
})
