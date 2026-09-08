// TurnTimeline page tests (§4.14). Mocks @/lib/tauri-api getTraceTimeline —
// no Tauri runtime involved. Covers: header/chips, turn cards with tool
// waterfall rows (incl. interrupted-call error marking), the cumulative
// curve card, the i18n-driven empty state, and the load-failure state.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import type * as TauriApi from '@/lib/tauri-api'
import { I18nProvider } from '@/i18n'
import TurnTimeline from '@/pages/TurnTimeline'
import type { TurnTimeline } from '@/types'

const getTraceTimeline = vi.hoisted(() => vi.fn())

vi.mock('@/lib/tauri-api', async () => {
  const actual = await vi.importActual<typeof TauriApi>('@/lib/tauri-api')
  return {
    ...actual,
    getTraceTimeline: (...args: unknown[]) => getTraceTimeline(...args),
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
})

describe('TurnTimeline', () => {
  it('renders header, summary chips, and both turn cards with tools', async () => {
    getTraceTimeline.mockResolvedValue(FIXTURE)
    renderAt()

    await waitFor(() => {
      expect(screen.getByText('Turn 1')).toBeInTheDocument()
    })
    expect(screen.getByRole('heading', { name: 'Turn Timeline' })).toBeInTheDocument()
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
