import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import { MemoryRouter } from 'react-router-dom'
import { save } from '@tauri-apps/plugin-dialog'
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

// Same local-day labelling the backend uses for by_day buckets (and the
// page's own localDayKey helper) — needed so the vs-yesterday hint can be
// derived against real calendar keys.
function dayKey(offsetDays: number): string {
  const d = new Date()
  d.setDate(d.getDate() - offsetDays)
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`
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

  // M5b (设计 06 KPI 第二行「较昨日 ±N%」): today vs yesterday, both from
  // by_day local-day buckets. (0.50 − 0.25) / 0.25 → +100%. No yesterday
  // bucket → the honest "—" (vs yesterday —), never a fabricated percent.
  it('derives the vs-yesterday hint on the Today-spend card from by_day', async () => {
    vi.mocked(api.getUsageStats).mockResolvedValue({
      ...fixture,
      by_day: [
        { label: dayKey(1), ...bucket, cost_usd: 0.25 },
        { label: dayKey(0), ...bucket, cost_usd: 0.5 },
      ],
    })
    renderUsage()

    await waitFor(() => expect(screen.getByTestId('usage-stat-today-cost')).toBeInTheDocument())
    expect(screen.getByTestId('usage-stat-today-cost')).toHaveTextContent('vs yesterday +100%')
  })

  it('shows a dash for the vs-yesterday hint when yesterday has no bucket', async () => {
    vi.mocked(api.getUsageStats).mockResolvedValue({
      ...fixture,
      by_day: [{ label: dayKey(0), ...bucket, cost_usd: 0.5 }],
    })
    renderUsage()

    await waitFor(() => expect(screen.getByTestId('usage-stat-today-cost')).toBeInTheDocument())
    expect(screen.getByTestId('usage-stat-today-cost')).toHaveTextContent('vs yesterday —')
  })

  // G3 (2026-10-15 parity review §2.3-2): token metrics own the first KPI
  // row; the two $ cards move to a second row and carry the unified「估算」
  // badge — DOM order is the reading order.
  it('orders token KPIs before the estimated-$ cards and badges the latter', async () => {
    vi.mocked(api.getUsageStats).mockResolvedValue(fixture)
    const { unmount } = renderUsage()

    const tokens = await screen.findByTestId('usage-stat-tokens')
    const estimatedCost = screen.getByTestId('usage-stat-estimated-cost')
    const todayCost = screen.getByTestId('usage-stat-today-cost')
    expect(tokens.compareDocumentPosition(estimatedCost) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    expect(estimatedCost.compareDocumentPosition(todayCost) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    // Both $ cards say they are estimates; token cards do not.
    expect(estimatedCost).toHaveTextContent('est.')
    expect(todayCost).toHaveTextContent('est.')
    unmount()
  })

  // G3 §2.3-4 honesty footnote: a nonzero unknown-rate count renders the
  // muted page-level note (both modes render it; zero/absent hides it).
  it('renders the unknown-rate footnote only when the count is positive', async () => {
    vi.mocked(api.getUsageStats).mockResolvedValue({ ...fixture, unknown_rate_records: 3 })
    const { unmount } = renderUsage()

    const note = await screen.findByTestId('usage-unknown-rate-note')
    expect(note).toHaveTextContent(/3 records used models with unknown rates/)
    unmount()

    vi.mocked(api.getUsageStats).mockResolvedValue({ ...fixture, unknown_rate_records: 0 })
    renderUsage()
    await waitFor(() => expect(screen.getByTestId('usage-stat-tokens')).toBeInTheDocument())
    expect(screen.queryByTestId('usage-unknown-rate-note')).not.toBeInTheDocument()
  })

  // M4 (设计 06 KPI 四卡之四「上下文峰值」): the session-index maximum of
  // SessionInfo.max_context_tokens, with the holding session's title as the
  // hint. No session ever logged a window → honest "—".
  //
  // T2 (R3 §4.1 峰值已用): when a session reports the real used numerator
  // (SessionInfo.peak_context_used_tokens), the card switches to the peak
  // USED with the （Z%） occupancy against that session's window in the
  // hint — the percentage form the R3 audit deferred until the numerator
  // landed. Sessions without the field still render the window-peak
  // absolute form below.
  it('renders the context-peak KPI card from the session index', async () => {
    vi.mocked(api.getUsageStats).mockResolvedValue(fixture)
    vi.mocked(api.listSessions).mockResolvedValue([
      {
        id: 'sess-a', title: 'Fix billing webhook', created_at: 0, message_count: 2,
        max_context_tokens: 200000,
      },
      {
        id: 'sess-b', title: 'Small chat', created_at: 0, message_count: 1,
        max_context_tokens: 96000,
      },
    ])
    renderUsage()

    const card = await screen.findByTestId('usage-stat-context-peak')
    expect(card).toHaveTextContent('Context peak')
    expect(card).toHaveTextContent('200K')
    expect(card).toHaveTextContent('Fix billing webhook')
    // 诚实契约反例:窗口在册但无已用分子 → 保持绝对值形态,无百分比。
    expect(card).not.toHaveTextContent('%')
    vi.mocked(api.listSessions).mockResolvedValue([]) // 还原,避免泄漏到其他用例
  })

  it('renders the peak-used percentage form when a session reports the numerator', async () => {
    vi.mocked(api.getUsageStats).mockResolvedValue(fixture)
    vi.mocked(api.listSessions).mockResolvedValue([
      {
        id: 'sess-a', title: 'Fix billing webhook', created_at: 0, message_count: 2,
        max_context_tokens: 200000, peak_context_used_tokens: 96000,
      },
      {
        id: 'sess-b', title: 'Small chat', created_at: 0, message_count: 1,
        max_context_tokens: 96000,
      },
    ])
    renderUsage()

    // The used numerator wins the card (48% of its own window); the hint
    // carries the design's （Z%） against THAT session's budget.
    const card = await screen.findByTestId('usage-stat-context-peak')
    expect(card).toHaveTextContent('Context peak')
    expect(card).toHaveTextContent('96K')
    expect(card).toHaveTextContent('Fix billing webhook · 48%')
    vi.mocked(api.listSessions).mockResolvedValue([]) // 还原,避免泄漏到其他用例
  })

  it('falls back to the plain title hint when the winning session has no window', async () => {
    vi.mocked(api.getUsageStats).mockResolvedValue(fixture)
    vi.mocked(api.listSessions).mockResolvedValue([
      {
        id: 'sess-a', title: 'Fix billing webhook', created_at: 0, message_count: 2,
        peak_context_used_tokens: 96000,
      },
    ])
    renderUsage()

    // Used numerator without a window budget: the value is real, the
    // percentage is not derivable — no fabricated %.
    const card = await screen.findByTestId('usage-stat-context-peak')
    expect(card).toHaveTextContent('96K')
    expect(card).toHaveTextContent('Fix billing webhook')
    expect(card).not.toHaveTextContent('%')
    vi.mocked(api.listSessions).mockResolvedValue([]) // 还原,避免泄漏到其他用例
  })

  it('shows a dash on the context-peak card when no session recorded a window', async () => {
    vi.mocked(api.getUsageStats).mockResolvedValue(fixture)
    renderUsage() // setup default: listSessions → []

    const card = await screen.findByTestId('usage-stat-context-peak')
    expect(card).toHaveTextContent('—')
    expect(card).toHaveTextContent('No session window recorded')
  })

  // M5a (设计 06:158 页头「导出 CSV」): one CSV with a `section` column —
  // the active range's by_day buckets plus the per-session detail — written
  // through the native save dialog (plugin-dialog save + save_text_file).
  it('exports by_day and per-session detail as one CSV via the save dialog', async () => {
    vi.mocked(api.getUsageStats).mockResolvedValue(fixture)
    vi.mocked(api.getUsageBySession).mockResolvedValue([
      {
        sessionId: 'sess-a', title: 'Fix billing webhook', inputTokens: 10, outputTokens: 5,
        cacheCreationTokens: 0, cacheReadTokens: 0, costUsd: 0.1, requests: 1,
        lastUsedAtMs: 1_700_000_000_000,
      },
    ])
    vi.mocked(save).mockResolvedValue('/tmp/shannon-usage.csv')
    renderUsage()

    await waitFor(() => expect(screen.getByText('claude-sonnet-4-6')).toBeInTheDocument())
    fireEvent.click(screen.getByRole('button', { name: /Export CSV/i }))
    await waitFor(() => expect(api.saveTextFile).toHaveBeenCalledWith(
      '/tmp/shannon-usage.csv',
      expect.stringContaining('section,label,input_tokens'),
    ))
    const csv = vi.mocked(api.saveTextFile).mock.calls[0][1]
    expect(csv).toContain(`by_day,2024-01-02,1000,500,100,50,0.25,3`)
    expect(csv).toContain('by_session,Fix billing webhook,10,5,0,0,0.1,1')
    vi.mocked(save).mockResolvedValue(null) // 还原 setup 默认(取消)
  })

  it('writes nothing when the save dialog is cancelled', async () => {
    vi.mocked(api.getUsageStats).mockResolvedValue(fixture)
    renderUsage()

    await waitFor(() => expect(screen.getByText('claude-sonnet-4-6')).toBeInTheDocument())
    fireEvent.click(screen.getByRole('button', { name: /Export CSV/i }))
    await waitFor(() => expect(save).toHaveBeenCalled())
    expect(api.saveTextFile).not.toHaveBeenCalled()
  })

  // 设计 06: the daily chart carries the Token/成本 metric toggle — the
  // cost metric renames the card to its own title so title = data. G3: the
  // cost title carries the (est.) marker — the $ series is an estimate.
  it('switches the daily chart between the Token and cost metric', async () => {
    vi.mocked(api.getUsageStats).mockResolvedValue(fixture)
    renderUsage()

    await waitFor(() => expect(screen.getByText('Daily tokens · 30 days')).toBeInTheDocument())
    fireEvent.click(screen.getByRole('button', { name: 'Cost' }))
    await waitFor(() => expect(screen.getByText('Daily cost (est.) · 30 days')).toBeInTheDocument())
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
