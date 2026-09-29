// TurnTimeline page tests (§4.14). Mocks @/lib/tauri-api getTraceTimeline —
// no Tauri runtime involved. Covers: subtitle row/summary chips, turn cards
// with tool waterfall rows (incl. interrupted-call error marking), the
// cumulative curve card, the i18n-driven empty state, and the load-failure
// state. (The page title itself lives in the Header's TITLE_MAP.)
// office Wave 3 C6 adds the Export-as-HTML flow (save dialog + saveTextFile).

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor, fireEvent } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import { save } from '@tauri-apps/plugin-dialog'
import type * as TauriApi from '@/lib/tauri-api'
import { I18nProvider } from '@/i18n'
import TurnTimeline from '@/pages/TurnTimeline'
import { timelineToHtml } from '@/lib/timelineExport'
import type { TurnTimeline } from '@/types'

const getTraceTimeline = vi.hoisted(() => vi.fn())
const saveTextFile = vi.hoisted(() => vi.fn())

vi.mock('@/lib/tauri-api', async () => {
  const actual = await vi.importActual<typeof TauriApi>('@/lib/tauri-api')
  return {
    ...actual,
    getTraceTimeline: (...args: unknown[]) => getTraceTimeline(...args),
    saveTextFile: (...args: unknown[]) => saveTextFile(...args),
  }
})

const BASE_NS = 1_756_200_000_000_000_000
const ns = (seconds: number) => BASE_NS + seconds * 1_000_000_000

const FIXTURE: TurnTimeline = {
  session_id: 'sess-001',
  model: 'claude-sonnet-4-20250514',
  started_ts_ns: ns(0),
  ended_ts_ns: ns(330),
  turns: [
    {
      turn: 1,
      start_ts_ns: ns(0),
      end_ts_ns: ns(120),
      reason: 'completed',
      input_tokens: 4820,
      output_tokens: 1240,
      cache_creation_tokens: 1200,
      cache_read_tokens: 3600,
      cost_usd: 0.0214,
      tools: [
        { tool_use_id: 'tu-001', tool_name: 'Read', start_ts_ns: ns(20), end_ts_ns: ns(26), duration_ms: 6000, is_error: false },
        { tool_use_id: 'tu-002', tool_name: 'Bash', start_ts_ns: ns(55), end_ts_ns: ns(58), duration_ms: 3000, is_error: true },
      ],
    },
    {
      turn: 2,
      start_ts_ns: ns(120),
      end_ts_ns: ns(330),
      reason: 'completed',
      input_tokens: 6110,
      output_tokens: 890,
      cache_creation_tokens: 0,
      cache_read_tokens: 4800,
      cost_usd: null,
      tools: [
        // Interrupted call — no measured duration; must still render as an error row.
        { tool_use_id: 'tu-003', tool_name: 'Grep', start_ts_ns: ns(145), end_ts_ns: ns(145), duration_ms: null, is_error: true },
      ],
    },
  ],
  cumulative: [
    { ts_ns: ns(120), output_tokens_total: 1240, cost_total_usd: 0.0214 },
    { ts_ns: ns(330), output_tokens_total: 2130, cost_total_usd: 0.0341 },
  ],
}

function renderAt(path = '/timeline/sess-001') {
  return render(
    <I18nProvider>
      <MemoryRouter initialEntries={[path]}>
        <Routes>
          <Route path="/timeline/:id" element={<TurnTimeline />} />
          <Route path="/chat" element={<div>chat-home</div>} />
        </Routes>
      </MemoryRouter>
    </I18nProvider>,
  )
}

beforeEach(() => {
  getTraceTimeline.mockReset()
  saveTextFile.mockReset()
  saveTextFile.mockResolvedValue(undefined)
  // The dialog `save` mock comes from the global setup (default: null, i.e.
  // the user cancels). Clear the call history the C6 tests assert against.
  vi.mocked(save).mockClear()
})

describe('TurnTimeline', () => {
  it('renders header, summary chips, and both turn cards with tools', async () => {
    getTraceTimeline.mockResolvedValue(FIXTURE)
    renderAt()

    await waitFor(() => {
      expect(screen.getByText('Turn 1')).toBeInTheDocument()
    })
    // The page-local h1 was converged into the Header's TITLE_MAP
    // (header.title.timeline) — the panel row carries the model subtitle.
    expect(screen.getByText('claude-sonnet-4-20250514')).toBeInTheDocument()
    expect(screen.getByText('Turn 2')).toBeInTheDocument()

    // Tool names across the waterfall rows.
    expect(screen.getByText('Read')).toBeInTheDocument()
    expect(screen.getByText('Bash')).toBeInTheDocument()
    expect(screen.getByText('Grep')).toBeInTheDocument()

    // Summary chip labels resolve through ICU plurals.
    expect(screen.getByLabelText('Session summary')).toHaveTextContent(
      /2 turns/,
    )
    expect(getTraceTimeline).toHaveBeenCalledWith('sess-001')
  })

  it('marks interrupted calls as errors and hides durations without measurements', async () => {
    getTraceTimeline.mockResolvedValue(FIXTURE)
    const { container } = renderAt()

    await screen.findByText('Turn 2')
    const grepRow = screen.getByTitle('Grep').closest('div') as HTMLElement
    // The row keeps the shared duration glyph but no numeric ms value.
    expect(grepRow.textContent).not.toMatch(/\d+(\.\d+)?s\b/)
    // Error styling class applied by the panel for is_error rows.
    expect(container.querySelectorAll('[class*="bg-error"]').length).toBeGreaterThan(0)
  })

  it('renders the cumulative curve card when samples exist', async () => {
    getTraceTimeline.mockResolvedValue(FIXTURE)
    renderAt()

    await screen.findByText('Accumulated tokens & cost')
    expect(screen.getByRole('img', { name: 'Token accumulation curve' })).toBeInTheDocument()
    expect(screen.getByText(/output tokens/)).toBeInTheDocument()
  })

  it('shows the empty state when the projection has no turns', async () => {
    getTraceTimeline.mockResolvedValue({
      ...FIXTURE,
      turns: [],
      cumulative: [],
    })
    renderAt()

    await screen.findByText('No turns recorded yet')
    expect(await screen.findByText(/Start a conversation/)).toBeInTheDocument()
  })

  it('shows the failure state and offers the way back to chat', async () => {
    getTraceTimeline.mockRejectedValue(new Error('Session not found: sess-404'))
    renderAt('/timeline/sess-404')

    await screen.findByText('Timeline unavailable')
    expect(screen.getByText(/no readable event log/)).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Back to chat' })).toBeInTheDocument()
  })

  it('passes an unknown route id straight to the API layer', async () => {
    getTraceTimeline.mockResolvedValue(FIXTURE)
    renderAt('/timeline/whatever-id')
    await waitFor(() => {
      expect(getTraceTimeline).toHaveBeenCalledWith('whatever-id')
    })
  })
})

// ─── B4 §7-28: reason badge tones, raw unknown reasons, app-locale times ───

function fixtureWithReason(reason: string): TurnTimeline {
  return {
    ...FIXTURE,
    cumulative: [],
    turns: [{ ...FIXTURE.turns[0], reason }],
  }
}

describe('TurnTimeline — reason badges and locale (B4 §7-28)', () => {
  it('renders a genuine failure reason in the error tone', async () => {
    getTraceTimeline.mockResolvedValue(fixtureWithReason('failed'))
    renderAt()
    const badge = await screen.findByText('Failed')
    // G7 2026-09-30: error tone = MD3 container pair (was bg-error/10 tint).
    expect(badge.className).toContain('bg-error-container')
  })

  it('renders neutral stopping reasons (interrupted) without error styling', async () => {
    getTraceTimeline.mockResolvedValue(fixtureWithReason('interrupted'))
    renderAt()
    const badge = await screen.findByText('Interrupted')
    // Neutral chip — not red: interrupted is not a failure.
    expect(badge.className).toContain('bg-surface-container-high')
    expect(badge.className).not.toContain('bg-error')
  })

  it('renders an unknown reason as its raw text, not a literal i18n key', async () => {
    getTraceTimeline.mockResolvedValue(fixtureWithReason('engine-restart'))
    renderAt()
    expect(await screen.findByText('engine-restart')).toBeInTheDocument()
    expect(screen.queryByText('timeline.reason.engine-restart')).not.toBeInTheDocument()
  })

  it('formats timestamps in the app locale (zh-CN), not the system default', async () => {
    window.localStorage.setItem('shannon.locale', 'zh-CN')
    try {
      getTraceTimeline.mockResolvedValue(fixtureWithReason('completed'))
      const { container } = renderAt()
      // zh-CN localizes the turn label — anchor on the testid instead.
      await screen.findByTestId('timeline-turn-1')
      const turn = FIXTURE.turns[0]
      const opts: Intl.DateTimeFormatOptions = { hour: '2-digit', minute: '2-digit', second: '2-digit' }
      const zhStart = new Intl.DateTimeFormat('zh-CN', opts).format(new Date(turn.start_ts_ns / 1e6))
      const enStart = new Intl.DateTimeFormat('en', opts).format(new Date(turn.start_ts_ns / 1e6))
      // The rendered time matches the zh-CN formatting…
      expect(container.textContent).toContain(zhStart)
      // …which must differ from what a system-default (en) render would show.
      expect(zhStart).not.toBe(enStart)
    } finally {
      window.localStorage.removeItem('shannon.locale')
    }
  })
})

// ─── office Wave 3 C6: Export as HTML (save dialog + saveTextFile) ───

describe('TurnTimeline — Export as HTML (office Wave 3 C6)', () => {
  it('the export button writes self-contained HTML with the step text to the chosen path', async () => {
    getTraceTimeline.mockResolvedValue(FIXTURE)
    vi.mocked(save).mockResolvedValueOnce('/tmp/export/timeline-sess-001.html')
    renderAt()

    await screen.findByText('Turn 1')
    fireEvent.click(screen.getByTestId('timeline-export-html'))
    await waitFor(() => {
      expect(saveTextFile).toHaveBeenCalledTimes(1)
    })
    const [path, html] = saveTextFile.mock.calls[0] as [string, string]
    expect(path).toBe('/tmp/export/timeline-sess-001.html')
    // Self-contained document carrying the timeline's step content.
    expect(html).toContain('<!DOCTYPE html>')
    expect(html).toContain('Turn 1')
    expect(html).toContain('Turn 2')
    expect(html).toContain('Read')
    expect(html).toContain('Grep')
    expect(html).toContain('sess-001')
    // Inline styles only — no scripts or external resources.
    expect(html).not.toContain('<script')
    expect(html).not.toContain('src=')
  })

  it('cancelling the save dialog never writes a file', async () => {
    getTraceTimeline.mockResolvedValue(FIXTURE)
    vi.mocked(save).mockResolvedValueOnce(null)
    renderAt()
    await screen.findByText('Turn 1')
    fireEvent.click(screen.getByTestId('timeline-export-html'))
    await waitFor(() => {
      expect(save).toHaveBeenCalledTimes(1)
    })
    expect(saveTextFile).not.toHaveBeenCalled()
  })

  it('timelineToHtml escapes HTML-sensitive tool names', () => {
    const html = timelineToHtml({
      ...FIXTURE,
      turns: [
        {
          ...FIXTURE.turns[0],
          tools: [
            { tool_use_id: 'tu-x', tool_name: '<script>', start_ts_ns: ns(1), end_ts_ns: ns(2), duration_ms: 10, is_error: false },
          ],
        },
      ],
    })
    expect(html).toContain('&lt;script&gt;')
    expect(html).not.toContain('<script>')
  })
})
