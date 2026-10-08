// TurnTimeline page tests (§4.14). Mocks @/lib/tauri-api getTraceTimeline —
// no Tauri runtime involved. Covers: subtitle row/summary chips, turn cards
// with tool waterfall rows (incl. interrupted-call error marking), the
// cumulative curve card, the i18n-driven empty state, and the load-failure
// state. (The page title itself lives in the Header's TITLE_MAP.)
// office Wave 3 C6 adds the Export-as-HTML flow; G5 P0-8 moved the save
// dialog + write into the backend (`saveTextFileViaDialog`).

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor, fireEvent } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import type * as TauriApi from '@/lib/tauri-api'
import { I18nProvider } from '@/i18n'
import TurnTimeline, { toolKind } from '@/pages/TurnTimeline'
import { timelineToHtml } from '@/lib/timelineExport'
import type { TurnTimeline } from '@/types'

const getTraceTimeline = vi.hoisted(() => vi.fn())
const saveTextFileViaDialog = vi.hoisted(() => vi.fn())
const listCheckpoints = vi.hoisted(() => vi.fn())
const rewindSessionApi = vi.hoisted(() => vi.fn())

vi.mock('@/lib/tauri-api', async () => {
  const actual = await vi.importActual<typeof TauriApi>('@/lib/tauri-api')
  return {
    ...actual,
    getTraceTimeline: (...args: unknown[]) => getTraceTimeline(...args),
    saveTextFileViaDialog: (...args: unknown[]) => saveTextFileViaDialog(...args),
    listCheckpoints: (...args: unknown[]) => listCheckpoints(...args),
    rewindSession: (...args: unknown[]) => rewindSessionApi(...args),
  }
})

// ChatContext/AppContext consumers: rewind routes through the chat slice
// when the timeline session IS the current one. Full module mocks — the
// page only consumes these two hooks.
const mockRewindCurrent = vi.hoisted(() => vi.fn())
const mockSwitchSession = vi.hoisted(() => vi.fn())
vi.mock('@/context/ChatContext', () => ({
  useChat: () => ({ rewindSession: mockRewindCurrent }),
}))
vi.mock('@/context/AppContext', () => ({
  useApp: () => ({
    currentSessionId: 'sess-001',
    switchSession: mockSwitchSession,
  }),
}))

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
    { ts_ns: ns(120), input_tokens_total: 4820, output_tokens_total: 1240, cost_total_usd: 0.0214 },
    { ts_ns: ns(330), input_tokens_total: 10930, output_tokens_total: 2130, cost_total_usd: 0.0341 },
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
  saveTextFileViaDialog.mockReset()
  listCheckpoints.mockReset()
  rewindSessionApi.mockReset()
  mockRewindCurrent.mockReset()
  mockSwitchSession.mockReset()
  // Default: no checkpoints — rewind chips stay hidden (honest affordance).
  listCheckpoints.mockResolvedValue([])
  // Default: the backend dialog "wrote" the file and reports the path.
  saveTextFileViaDialog.mockResolvedValue('/tmp/export/timeline-sess-001.html')
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

    // Tool names across the waterfall rows ('Read' also matches the
    // four-color legend's Read item, so it asserts on "any occurrence").
    expect(screen.getAllByText('Read').length).toBeGreaterThan(0)
    expect(screen.getByText('Bash')).toBeInTheDocument()
    expect(screen.getByText('Grep')).toBeInTheDocument()

    // Summary chip labels resolve through ICU plurals. The chips live in a
    // role="list" container (office wave 3 moved the section title into the
    // persistent Header, removing the old "Session summary" label); the
    // four-color legend is a separate list below.
    expect(screen.getByRole('list', { name: 'Session summary' })).toHaveTextContent(/2 turns/)
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

// ─── office Wave 3 C6: Export as HTML (G5 P0-8 backend dialog flow) ───

describe('TurnTimeline — Export as HTML (office Wave 3 C6)', () => {
  it('exports self-contained HTML through the backend save-dialog command', async () => {
    getTraceTimeline.mockResolvedValue(FIXTURE)
    saveTextFileViaDialog.mockResolvedValueOnce('/home/user/Downloads/timeline-sess-001.html')
    renderAt()

    await screen.findByText('Turn 1')
    fireEvent.click(screen.getByTestId('timeline-export-html'))
    await waitFor(() => {
      expect(saveTextFileViaDialog).toHaveBeenCalledTimes(1)
    })
    const [html, defaultName] = saveTextFileViaDialog.mock.calls[0] as [string, string]
    expect(defaultName).toBe('timeline-sess-001.html')
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
    saveTextFileViaDialog.mockResolvedValueOnce(null)
    renderAt()
    await screen.findByText('Turn 1')
    fireEvent.click(screen.getByTestId('timeline-export-html'))
    await waitFor(() => {
      expect(saveTextFileViaDialog).toHaveBeenCalledTimes(1)
    })
    // Cancel (null) backs out silently — no toast, no retry.
    expect(saveTextFileViaDialog).toHaveBeenCalledTimes(1)
  })

  it('a backend write failure surfaces as the export-failed toast path', async () => {
    getTraceTimeline.mockResolvedValue(FIXTURE)
    saveTextFileViaDialog.mockRejectedValueOnce(new Error('disk full'))
    renderAt()
    await screen.findByText('Turn 1')
    fireEvent.click(screen.getByTestId('timeline-export-html'))
    // Must not throw out of the handler; the catch turns it into a toast.
    await waitFor(() => {
      expect(saveTextFileViaDialog).toHaveBeenCalledTimes(1)
    })
    expect(screen.getByTestId('timeline-export-html')).toBeEnabled()
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

// ─── audit §10 P1: checkpoint rewind chips ───

describe('TurnTimeline — rewind chips (audit §10 P1)', () => {
  it('hides the rewind chips when no checkpoint covers any turn', async () => {
    getTraceTimeline.mockResolvedValue(FIXTURE)
    listCheckpoints.mockResolvedValue([])
    renderAt()
    await screen.findByText('Turn 1')
    expect(screen.queryByTestId('timeline-rewind-1')).not.toBeInTheDocument()
    expect(screen.queryByTestId('timeline-rewind-2')).not.toBeInTheDocument()
  })

  it('shows the chip for every turn a checkpoint at-or-after covers', async () => {
    getTraceTimeline.mockResolvedValue(FIXTURE)
    // One checkpoint at turn 2 covers turns 1 AND 2 (rewinding to before
    // turn 1 is legitimate while any later checkpoint exists).
    listCheckpoints.mockResolvedValue([{ turn_index: 2 }])
    renderAt()
    await screen.findByText('Turn 1')
    expect(screen.getByTestId('timeline-rewind-1')).toBeInTheDocument()
    expect(screen.getByTestId('timeline-rewind-2')).toBeInTheDocument()
  })

  it('rewinds the CURRENT session through the chat slice and navigates to chat', async () => {
    getTraceTimeline.mockResolvedValue(FIXTURE)
    listCheckpoints.mockResolvedValue([
      { turn_index: 1 },
      { turn_index: 2 },
    ])
    mockRewindCurrent.mockResolvedValue([])
    renderAt()
    await screen.findByText('Turn 1')
    fireEvent.click(screen.getByTestId('timeline-rewind-1'))
    // Confirm dialog gates the destructive action.
    fireEvent.click(await screen.findByRole('button', { name: 'Rewind' }))
    await waitFor(() => expect(mockRewindCurrent).toHaveBeenCalledWith(1))
    expect(rewindSessionApi).not.toHaveBeenCalled()
    expect(mockSwitchSession).not.toHaveBeenCalled()
    await waitFor(() => expect(screen.getByText('chat-home')).toBeInTheDocument())
  })

  it('rewinds a NON-current session through the raw command, then adopts it', async () => {
    getTraceTimeline.mockResolvedValue(FIXTURE)
    listCheckpoints.mockResolvedValue([{ turn_index: 1 }])
    rewindSessionApi.mockResolvedValue([])
    renderAt('/timeline/sess-other')
    await screen.findByText('Turn 1')
    fireEvent.click(screen.getByTestId('timeline-rewind-1'))
    fireEvent.click(await screen.findByRole('button', { name: 'Rewind' }))
    await waitFor(() => expect(rewindSessionApi).toHaveBeenCalledWith('sess-other', 1))
    expect(mockRewindCurrent).not.toHaveBeenCalled()
    await waitFor(() => expect(mockSwitchSession).toHaveBeenCalledWith('sess-other'))
    await waitFor(() => expect(screen.getByText('chat-home')).toBeInTheDocument())
  })
})

// ─── 裁决 B7: coarse four-color waterfall ───

describe('toolKind (B7 coarse frontend binning)', () => {
  it('bins tool names into read/write/net/risk', () => {
    expect(toolKind('Grep')).toBe('read')
    expect(toolKind('Read')).toBe('read')
    expect(toolKind('Glob')).toBe('read')
    expect(toolKind('LS')).toBe('read')
    expect(toolKind('Edit')).toBe('write')
    expect(toolKind('Write')).toBe('write')
    expect(toolKind('MultiEdit')).toBe('write')
    expect(toolKind('WebFetch')).toBe('net')
    expect(toolKind('mcp__tools__browser_navigate')).toBe('net')
    expect(toolKind('Bash')).toBe('risk')
    expect(toolKind('mcp__term__terminal_run')).toBe('risk')
    expect(toolKind('TodoWrite')).toBe('write')
    expect(toolKind('Task')).toBe('other')
  })

  it('renders a failed read as the error red, not the kind tint', async () => {
    getTraceTimeline.mockResolvedValue(FIXTURE)
    renderAt()
    await screen.findByText('Turn 1')
    const bashBar = screen.getByTitle('Bash · 3.0s')
    expect(bashBar.className).toContain('bg-error/15')
    const readBar = screen.getByTitle('Read · 6.0s')
    expect(readBar.className).toContain('bg-info/15')
    expect(readBar.className).not.toContain('bg-error')
  })

  it('renders the four-kind legend', async () => {
    getTraceTimeline.mockResolvedValue(FIXTURE)
    renderAt()
    const legend = await screen.findByRole('list', { name: 'Waterfall color legend' })
    expect(legend.textContent).toMatch(/Read/)
    expect(legend.textContent).toMatch(/Write/)
    expect(legend.textContent).toMatch(/Network/)
    expect(legend.textContent).toMatch(/High risk/)
  })
})

// ─── audit §10 P2/C6: KPI chips + input leg on the cumulative curve ───

describe('TurnTimeline — KPI chips and input curve (§10 P2, C6)', () => {
  it('shows the wall-clock duration chip and in+out token chip', async () => {
    getTraceTimeline.mockResolvedValue(FIXTURE)
    renderAt()
    const list = await screen.findByRole('list', { name: 'Session summary' })
    // 330s span → "5m30s"; tokens: 4820+6110=10930 in / 2130 out.
    expect(list).toHaveTextContent('5m30s')
    expect(list.textContent).toMatch(/10\.9K/)
    expect(list.textContent).toMatch(/2\.1K/)
  })

  it('draws the input leg plus in/out legend when input data exists', async () => {
    getTraceTimeline.mockResolvedValue(FIXTURE)
    renderAt()
    await screen.findByText('Accumulated tokens & cost')
    const svg = screen.getByRole('img', { name: 'Token accumulation curve' })
    // Output + input + cost polylines (the fixture carries costs).
    expect(svg.querySelectorAll('polyline').length).toBe(3)
    expect(screen.getByText('Output')).toBeInTheDocument()
    expect(screen.getByText('Input')).toBeInTheDocument()
  })

  it('hides the input leg when the projection predates the field', async () => {
    getTraceTimeline.mockResolvedValue({
      ...FIXTURE,
      cumulative: FIXTURE.cumulative.map(p => ({ ...p, input_tokens_total: 0 })),
    })
    renderAt()
    await screen.findByText('Accumulated tokens & cost')
    const svg = screen.getByRole('img', { name: 'Token accumulation curve' })
    // Output + cost only.
    expect(svg.querySelectorAll('polyline').length).toBe(2)
    expect(screen.queryByText('Input')).not.toBeInTheDocument()
  })
})
