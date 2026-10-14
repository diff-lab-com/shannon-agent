// 补1 (R2 审查 §7.1 · 02-chat msg-meta) — assistant turn aggregate meta in
// MessageBubble's header: 「N 个工具调用 · N 分钟」 derived from data the
// bubble already holds. Honesty contract under test: a dimension that cannot
// be derived renders NOTHING — never a 0, never a placeholder; turn-level
// cost has no reachable data source, so no cost segment exists at all.
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

import { MessageBubble, turnDurationValue } from '@/components/chat/MessageBubble'
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
 *  how MessageArea hands bubbles their rows). */
function renderTurn(messages: ChatMessage[], index: number) {
  useChatMock.mockImplementation(() => makeCtx(messages))
  return render(
    wrap(<MessageBubble message={messages[index]} messageIndex={index} onViewDiff={vi.fn()} />),
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
