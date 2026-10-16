// 补1 (R2 审查 §7.1 · 02-chat msg-meta) — assistant turn aggregate meta in
// MessageBubble's header: 「N 个工具调用 · 2.4 分钟 · $0.087」 derived from data
// the bubble already holds. Honesty contract under test: a dimension that
// cannot be derived renders NOTHING — never a 0, never a placeholder.
// R3 §4 遗留项: turn COST joined the tail (TimelineTurn.cost_usd projected
// into TurnCostWindows by MessageArea; matched by timestamp overlap), and a
// successfully completed assistant turn wears the neutral 「完成/Done」 chip
// at the tail's head (failed/stopped turns keep their bubble-foot markers
// and get no chip).
//
// Quadrants covered:
//   tools + duration | tools + no duration
//   no tools + duration | no tools + no duration
// plus sub-second omission, user/tool exclusion, multi-assistant turn
// aggregation, turn boundary (next user closes the window), the a11y
// aria-label 汇总, and the zh-CN copy.

import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { IntlProvider } from 'react-intl'

import { MessageBubble, matchTurnCost, turnCostValue, turnDurationValue, type TurnCostWindow } from '@/components/chat/MessageBubble'
import type { ChatMessage, ToolCall } from '@/types'
import zhCN from '@/i18n/locales/zh-CN.json'

const useChatMock = vi.hoisted(() => vi.fn())

vi.mock('@/context/ChatContext', () => ({
  useChat: useChatMock,
}))
vi.mock('@/context/SessionContext', () => ({
  useSessions: () => ({
    currentSessionId: 's-1',
    switchSession: vi.fn(),
    refreshSessions: vi.fn(),
  }),
}))

function makeCtx(messages: ChatMessage[]) {
  return {
    messages,
    sendMessage: vi.fn().mockResolvedValue(true),
    feedback: {},
    recordFeedback: vi.fn().mockResolvedValue(undefined),
    isQuerying: false,
  }
}

const wrap = (ui: React.ReactNode) => <MemoryRouter>{ui}</MemoryRouter>

const BASE = Date.UTC(2026, 0, 1, 12, 0, 0)

const user = (timestamp: number, overrides: Partial<ChatMessage> = {}): ChatMessage => ({
  role: 'user',
  content: 'question',
  timestamp,
  ...overrides,
})

const assistant = (
  timestamp: number,
  overrides: Partial<ChatMessage> = {},
): ChatMessage => ({
  role: 'assistant',
  content: 'answer',
  timestamp,
  ...overrides,
})

const call = (id: string, extra: Partial<ToolCall> = {}): ToolCall => ({
  tool_use_id: id,
  tool_name: 'read_file',
  tool_input: {},
  status: 'completed',
  is_error: false,
  ...extra,
})

/** Render the message at `index` of `messages` (identity-preserved, exactly
 *  how MessageArea hands bubbles their rows). `extra` carries the optional
 *  lookup props (costWindows) MessageArea derives alongside durationLookup. */
function renderTurn(messages: ChatMessage[], index: number, extra: { costWindows?: TurnCostWindow[] } = {}) {
  useChatMock.mockImplementation(() => makeCtx(messages))
  return render(
    wrap(
      <MessageBubble
        message={messages[index]}
        messageIndex={index}
        onViewDiff={vi.fn()}
        {...extra}
      />,
    ),
  )
}

describe('turnDurationValue (pure derivation)', () => {
  it('drops non-deriveable inputs instead of producing a 0', () => {
    expect(turnDurationValue(0)).toBeNull()
    expect(turnDurationValue(-400)).toBeNull()
    expect(turnDurationValue(Number.NaN)).toBeNull()
    expect(turnDurationValue(Number.POSITIVE_INFINITY)).toBeNull()
  })

  it('drops sub-second spans that round to 0 (below the displayed granularity)', () => {
    expect(turnDurationValue(400)).toBeNull()
    expect(turnDurationValue(499)).toBeNull()
    // 999ms legitimately rounds to a displayed 1 second.
    expect(turnDurationValue(999)).toEqual({ seconds: 1 })
  })

  it('formats whole seconds under a minute', () => {
    expect(turnDurationValue(1000)).toEqual({ seconds: 1 })
    expect(turnDurationValue(45_600)).toEqual({ seconds: 46 })
  })

  it('switches to one-decimal minutes at 60s', () => {
    expect(turnDurationValue(60_000)).toEqual({ minutes: 1 })
    // 2.4 分钟 — the design's exact figure
    expect(turnDurationValue(144_000)).toEqual({ minutes: 2.4 })
    expect(turnDurationValue(3_595_000)).toEqual({ minutes: 59.9 })
  })
})

describe('MessageBubble turn meta — the four quadrants', () => {
  it('tools + duration renders both segments', () => {
    const messages = [
      user(BASE),
      assistant(BASE + 144_000, { tool_calls: [call('a'), call('b'), call('c')] }),
    ]
    renderTurn(messages, 1)
    const meta = screen.getByTestId('turn-meta')
    expect(meta).toHaveTextContent('3 tool calls')
    expect(meta).toHaveTextContent('2.4 minutes')
  })

  it('tools + no duration (equal timestamps → zero span) renders only the count', () => {
    const messages = [
      user(BASE),
      assistant(BASE, { tool_calls: [call('a')] }),
    ]
    renderTurn(messages, 1)
    const meta = screen.getByTestId('turn-meta')
    expect(meta).toHaveTextContent('1 tool call')
    expect(meta).not.toHaveTextContent('second')
    expect(meta).not.toHaveTextContent('minute')
  })

  it('no tools + duration renders only the duration', () => {
    const messages = [
      user(BASE),
      assistant(BASE + 90_000),
    ]
    renderTurn(messages, 1)
    const meta = screen.getByTestId('turn-meta')
    expect(meta).toHaveTextContent('1.5 minutes')
    expect(meta).not.toHaveTextContent('tool call')
  })

  it('no tools + no duration renders no meta at all (整段不渲染)', () => {
    const messages = [
      user(BASE),
      assistant(BASE),
    ]
    renderTurn(messages, 1)
    expect(screen.queryByTestId('turn-meta')).toBeNull()
  })
})

describe('MessageBubble turn meta — honesty edges', () => {
  it('omits the duration segment for sub-second turns (never shows 0)', () => {
    const messages = [
      user(BASE),
      assistant(BASE + 400, { tool_calls: [call('a')] }),
    ]
    renderTurn(messages, 1)
    const meta = screen.getByTestId('turn-meta')
    expect(meta).toHaveTextContent('1 tool call')
    expect(meta).not.toHaveTextContent('0')
    expect(meta).not.toHaveTextContent('second')
  })

  it('ignores invalid (0 / non-finite) timestamps', () => {
    const messages = [
      user(0),
      assistant(Number.NaN as unknown as number, { tool_calls: [call('a')] }),
    ]
    renderTurn(messages, 1)
    const meta = screen.getByTestId('turn-meta')
    expect(meta).toHaveTextContent('1 tool call')
    expect(meta).not.toHaveTextContent('minute')
  })

  it('renders nothing on user messages', () => {
    const messages = [
      user(BASE, { tool_calls: undefined }),
      assistant(BASE + 60_000, { tool_calls: [call('a')] }),
    ]
    renderTurn(messages, 0)
    expect(screen.queryByTestId('turn-meta')).toBeNull()
  })

  it('renders nothing on tool-role messages', () => {
    const messages: ChatMessage[] = [
      user(BASE),
      assistant(BASE + 60_000, { tool_calls: [call('a')] }),
      { role: 'tool', content: 'tool ran', timestamp: BASE + 61_000 },
    ]
    renderTurn(messages, 2)
    expect(screen.queryByTestId('turn-meta')).toBeNull()
  })
})

describe('MessageBubble turn meta — turn scoping', () => {
  it('aggregates across several assistant messages of the same turn', () => {
    const messages = [
      user(BASE),
      assistant(BASE + 30_000, { tool_calls: [call('a')] }),
      assistant(BASE + 150_000, { tool_calls: [call('b'), call('c')] }),
    ]
    // First assistant of the turn — full turn totals, not just its own.
    const { unmount } = renderTurn(messages, 1)
    expect(screen.getByTestId('turn-meta')).toHaveTextContent('3 tool calls')
    expect(screen.getByTestId('turn-meta')).toHaveTextContent('2.5 minutes')
    unmount()
    // Last assistant of the turn — the same turn totals.
    renderTurn(messages, 2)
    expect(screen.getByTestId('turn-meta')).toHaveTextContent('3 tool calls')
    expect(screen.getByTestId('turn-meta')).toHaveTextContent('2.5 minutes')
  })

  it('the next user message closes the turn window', () => {
    const messages = [
      user(BASE),
      assistant(BASE + 120_000, { tool_calls: [call('a'), call('b'), call('c'), call('d'), call('e')] }),
      user(BASE + 200_000),
      assistant(BASE + 230_000),
    ]
    // The second turn's answer: its own turn only — 0 calls, 30s span.
    renderTurn(messages, 3)
    const meta = screen.getByTestId('turn-meta')
    expect(meta).not.toHaveTextContent('tool call')
    expect(meta).toHaveTextContent('30 seconds')
  })

  it('falls back to the single message when the context list is not this row', () => {
    const message = assistant(BASE + 60_000, { tool_calls: [call('a'), call('b')] })
    useChatMock.mockImplementation(() => makeCtx([]))
    render(wrap(<MessageBubble message={message} messageIndex={0} onViewDiff={vi.fn()} />))
    const meta = screen.getByTestId('turn-meta')
    expect(meta).toHaveTextContent('2 tool calls')
    // No pair to span against → no duration.
    expect(meta).not.toHaveTextContent('minute')
  })
})

describe('MessageBubble turn meta — a11y 汇总 + zh-CN copy', () => {
  it('exposes one coherent aria-label instead of bare fragments', () => {
    const messages = [
      user(BASE),
      assistant(BASE + 144_000, { tool_calls: [call('a'), call('b'), call('c')] }),
    ]
    renderTurn(messages, 1)
    expect(
      screen.getByRole('group', { name: 'This turn: 3 tool calls · 2.4 minutes' }),
    ).toBeInTheDocument()
    // Visual fragments are decorative; the group's label is the content.
    expect(screen.getByTestId('turn-meta')).toHaveAttribute('aria-label', 'This turn: 3 tool calls · 2.4 minutes')
  })

  it('aria-label carries the single derivable dimension alone', () => {
    const messages = [
      user(BASE),
      assistant(BASE, { tool_calls: [call('a')] }),
    ]
    renderTurn(messages, 1)
    expect(screen.getByTestId('turn-meta')).toHaveAttribute('aria-label', 'This turn: 1 tool call')
  })

  it('renders the zh-CN copy 「3 个工具调用 · 2.4 分钟」 from the design', () => {
    const messages = [
      user(BASE),
      assistant(BASE + 144_000, { tool_calls: [call('a'), call('b'), call('c')] }),
    ]
    useChatMock.mockImplementation(() => makeCtx(messages))
    render(
      wrap(
        // Nested provider overrides the auto-wrapped en provider for this
        // subtree — the same messages the app loads for zh-CN.
        <IntlProvider locale="zh-CN" defaultLocale="en" messages={zhCN as Record<string, string>}>
          <MessageBubble message={messages[1]} messageIndex={1} onViewDiff={vi.fn()} />
        </IntlProvider>,
      ),
    )
    const meta = screen.getByTestId('turn-meta')
    expect(meta).toHaveTextContent('3 个工具调用')
    expect(meta).toHaveTextContent('2.4 分钟')
    expect(meta).toHaveAttribute('aria-label', '本回合：3 个工具调用 · 2.4 分钟')
  })
})

// R3 §4 遗留项 — the cost segment. Data path under test: MessageArea projects
// TimelineTurn.cost_usd (trace_timeline IPC) into TurnCostWindows keyed by
// epoch-ms windows; the bubble matches its turn window by timestamp OVERLAP
// (matchTurnCost — rewind-proof, unlike the engine's envelope turn numbers).
// Honesty contract: absent/0/non-finite costs never produce a window
// upstream, and turnCostValue refuses them again at the render boundary.
describe('MessageBubble turn meta — cost segment (R3 §4)', () => {
  const OVERLAPPING: TurnCostWindow[] = [
    { startMs: BASE - 1_000, endMs: BASE + 145_000, costUsd: 0.087 },
  ]

  it('renders the design figure $0.087 AFTER the duration segment when a window overlaps', () => {
    const messages = [
      user(BASE),
      assistant(BASE + 144_000, { tool_calls: [call('a'), call('b'), call('c')] }),
    ]
    renderTurn(messages, 1, { costWindows: OVERLAPPING })
    const meta = screen.getByTestId('turn-meta')
    expect(meta).toHaveTextContent('$0.087')
    expect(meta).toHaveTextContent('2.4 minutes')
    // 追加语义: cost rides after the duration, tools stay first.
    const text = meta.textContent ?? ''
    expect(text.indexOf('tool calls')).toBeGreaterThan(-1)
    expect(text.indexOf('2.4 minutes')).toBeGreaterThan(text.indexOf('tool calls'))
    expect(text.indexOf('$0.087')).toBeGreaterThan(text.indexOf('2.4 minutes'))
    // The cost joins the aria 汇总 too. G3 (2026-10-15 parity review): the
    // figure is an estimate — the segment reads ≈$0.087.
    expect(meta).toHaveAttribute('aria-label', 'This turn: 3 tool calls · 2.4 minutes · ≈$0.087')
  })

  it('omits the cost segment when no window overlaps the turn (反例: absent)', () => {
    const messages = [
      user(BASE),
      assistant(BASE + 144_000, { tool_calls: [call('a')] }),
    ]
    renderTurn(messages, 1)
    const meta = screen.getByTestId('turn-meta')
    expect(meta).toHaveTextContent('1 tool call')
    expect(meta).not.toHaveTextContent('$')
  })

  it('omits the cost segment when the only window lies OUTSIDE the turn', () => {
    const messages = [
      user(BASE),
      assistant(BASE + 144_000, { tool_calls: [call('a')] }),
    ]
    renderTurn(messages, 1, {
      costWindows: [{ startMs: BASE + 1_000_000, endMs: BASE + 1_100_000, costUsd: 0.5 }],
    })
    const meta = screen.getByTestId('turn-meta')
    expect(meta).toHaveTextContent('1 tool call')
    expect(meta).not.toHaveTextContent('$')
  })

  it('cost alone (no tools, sub-second span) still earns its segment', () => {
    const messages = [
      user(BASE),
      assistant(BASE + 400),
    ]
    renderTurn(messages, 1, { costWindows: OVERLAPPING })
    const meta = screen.getByTestId('turn-meta')
    expect(meta).toHaveTextContent('$0.087')
    expect(meta).not.toHaveTextContent('second')
    expect(meta).not.toHaveTextContent('tool call')
  })

  it('turnCostValue refuses zero/negative/non-finite costs (never a $0.000)', () => {
    expect(turnCostValue(0)).toBeNull()
    expect(turnCostValue(-0.087)).toBeNull()
    expect(turnCostValue(Number.NaN)).toBeNull()
    expect(turnCostValue(Number.POSITIVE_INFINITY)).toBeNull()
    // The design's three decimals under a dollar; plain currency above.
    expect(turnCostValue(0.087)).toBe('$0.087')
    expect(turnCostValue(1.2)).toBe('$1.20')
  })

  it('matchTurnCost uses overlap (not containment) and takes the first hit', () => {
    const windows: TurnCostWindow[] = [
      { startMs: BASE + 5_000, endMs: BASE + 50_000, costUsd: 0.01 },
      { startMs: BASE + 60_000, endMs: BASE + 200_000, costUsd: 0.02 },
    ]
    // Containment would fail the first window (turn starts before it) —
    // overlap matches it, mirroring real optimistic-message/event drift.
    expect(matchTurnCost(windows, BASE, BASE + 10_000)).toBe(0.01)
    expect(matchTurnCost(windows, BASE + 100_000, BASE + 150_000)).toBe(0.02)
    expect(matchTurnCost(windows, BASE + 300_000, BASE + 400_000)).toBeNull()
    expect(matchTurnCost(undefined, BASE, BASE + 1)).toBeNull()
    expect(matchTurnCost(null, BASE, BASE + 1)).toBeNull()
  })
})

// R3 §4 遗留项 — the neutral 「完成/Done」 chip at the meta tail's head
// (02 页 msg-meta 首段 `st ok 完成`, rendered in the codebase's neutral form
// — no green). 正例: every successfully completed assistant turn wears it.
// 反例: failed/stopped turns keep their bubble-foot markers and get NO chip.
describe('MessageBubble turn meta — 完成 chip (R3 §4)', () => {
  it('renders the neutral Done chip on a successful assistant turn', () => {
    const messages = [
      user(BASE),
      assistant(BASE + 144_000, { tool_calls: [call('a')] }),
    ]
    renderTurn(messages, 1)
    const chip = screen.getByTestId('turn-meta-done')
    expect(chip).toHaveTextContent('Done')
    // 中性色非绿色: the chip shares the stopped-marker's neutral surface pair.
    expect(chip.className).toContain('bg-surface-container-high')
    expect(chip.className).toContain('text-on-surface-variant')
    expect(chip.className).not.toContain('text-success')
  })

  it('renders the chip even when no numeric meta is derivable (group stays absent)', () => {
    const messages = [
      user(BASE),
      assistant(BASE), // zero span, no tools, no cost — groupless turn
    ]
    renderTurn(messages, 1)
    expect(screen.getByTestId('turn-meta-done')).toHaveTextContent('Done')
    expect(screen.queryByTestId('turn-meta')).toBeNull()
  })

  it('renders NO chip on a failed turn (the failed marker plays that role)', () => {
    const messages = [
      user(BASE),
      assistant(BASE + 60_000, {
        interrupted: true,
        interrupted_reason: 'failed',
      }),
    ]
    renderTurn(messages, 1)
    expect(screen.queryByTestId('turn-meta-done')).toBeNull()
    expect(screen.getByTestId('message-failed-marker')).toBeInTheDocument()
  })

  it('renders NO chip on a stopped (cancelled) turn (不重复叠加)', () => {
    const messages = [
      user(BASE),
      assistant(BASE + 60_000, {
        interrupted: true,
        interrupted_reason: 'cancelled',
      }),
    ]
    renderTurn(messages, 1)
    expect(screen.queryByTestId('turn-meta-done')).toBeNull()
    expect(screen.getByTestId('message-stopped-marker')).toBeInTheDocument()
  })

  it('renders NO chip on user or tool messages', () => {
    const messages: ChatMessage[] = [
      user(BASE),
      assistant(BASE + 60_000, { tool_calls: [call('a')] }),
      { role: 'tool', content: 'tool ran', timestamp: BASE + 61_000 },
    ]
    const first = renderTurn(messages, 0)
    expect(screen.queryByTestId('turn-meta-done')).toBeNull()
    first.unmount()
    useChatMock.mockImplementation(() => makeCtx(messages))
    render(wrap(<MessageBubble message={messages[2]} messageIndex={2} onViewDiff={vi.fn()} />))
    expect(screen.queryByTestId('turn-meta-done')).toBeNull()
  })
})
