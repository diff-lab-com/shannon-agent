// Settings R3 T9 — history/committed thinking blocks in MessageBubble,
// gated by the three-tier display pref (lib/thinkingPref): 'all' (default)
// renders every non-empty block collapsed; 'first' only on the assistant
// message the list parent flagged first-of-turn; 'none' hides everywhere.
// A message without thinking never renders a block, whatever the tier.
// R3 §4 (Reasoning 头分态): a committed block is a SETTLED turn — its header
// is the COMPLETED state 「已思考/Thought」 (duration-less: no wire field
// carries reasoning block timestamps yet, see reasoningDurationValue), never
// the live 「思考中…/Thinking…」 label.
//
// T1 (2026-10-15, R3 §4.1 Reasoning 时长): the TURN-level duration landed —
// TimelineTurn.reasoning_duration_ms rides the trace-timeline IPC into the
// bubble as `reasoningWindows` (MessageArea's projection). The header then
// upgrades to 「已思考 N 秒/Thought for N seconds」 — ONLY when a window
// overlaps the turn; absent/sub-second durations keep the duration-less
// fallback (honesty contract, never a 0).

import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { I18nProvider } from '@/i18n'
import { MessageBubble, reasoningDurationValue, reasoningDurationFromMs, matchTurnReasoning, type TurnReasoningWindow } from '@/components/chat/MessageBubble'
import { ChatContext, type ChatContextValue } from '@/context/ChatContext'
import { SessionContext, type SessionContextValue } from '@/context/SessionContext'
import { CatalogContext, type CatalogContextValue } from '@/context/CatalogContext'
import { SHOW_THINKING_PREF_KEY } from '@/lib/thinkingPref'
import type { ChatMessage } from '@/types'

const sessionValue = {
  sessions: [],
  sessionActivity: {},
  goalRunsBySession: {},
  subagentLive: null,
  currentSessionId: 's-current',
  windowSessionId: null,
  switchingSession: false,
  switchSession: vi.fn().mockResolvedValue(undefined),
} as unknown as SessionContextValue

const chatValue = {
  sendMessage: vi.fn().mockResolvedValue(true),
  cancelQuery: vi.fn().mockResolvedValue(undefined),
  promptQueue: [],
  enqueuePrompt: vi.fn(() => true),
  dequeuePrompt: vi.fn(() => null),
  removeQueuedPrompt: vi.fn(),
  moveQueuedPrompt: vi.fn(),
  checkpoints: [],
  rewindSession: vi.fn().mockResolvedValue(undefined),
  compactSession: vi.fn(),
  feedback: {},
  recordFeedback: vi.fn().mockResolvedValue(undefined),
  contextPanelOpen: false,
  toggleContextPanel: vi.fn(),
  setContextPanelOpen: vi.fn(),
} as unknown as ChatContextValue

const catalogValue = {
  config: null,
} as unknown as CatalogContextValue

function providers({ children }: { children: React.ReactNode }) {
  return (
    <MemoryRouter initialEntries={['/']}>
      <ChatContext.Provider value={chatValue}>
        <SessionContext.Provider value={sessionValue}>
          <CatalogContext.Provider value={catalogValue}>
            <I18nProvider>{children}</I18nProvider>
          </CatalogContext.Provider>
        </SessionContext.Provider>
      </ChatContext.Provider>
    </MemoryRouter>
  )
}

function assistantMessage(withThinking: ChatMessage['thinking']): ChatMessage {
  return {
    role: 'assistant',
    content: 'Answer text',
    timestamp: 1_700_000_000_000,
    ...(withThinking != null ? { thinking: withThinking } : {}),
  }
}

function renderBubble(
  message: ChatMessage,
  isFirstOfTurn = false,
  extra: { reasoningWindows?: TurnReasoningWindow[] } = {},
) {
  return render(
    <MessageBubble
      message={message}
      messageIndex={1}
      isFirstAssistantOfTurn={isFirstOfTurn}
      onViewDiff={vi.fn()}
      {...extra}
    />,
    { wrapper: providers },
  )
}

const setPref = (pref: 'all' | 'first' | 'none' | null) => {
  if (pref === null) window.localStorage.removeItem(SHOW_THINKING_PREF_KEY)
  else window.localStorage.setItem(SHOW_THINKING_PREF_KEY, pref)
}

describe('MessageBubble thinking display (Settings R3 T9)', () => {
  beforeEach(() => {
    window.localStorage.clear()
  })

  it('renders the collapsed thinking block under the default "all" tier', () => {
    setPref(null)
    renderBubble(assistantMessage('Let me think about this'))
    expect(screen.getByText('Thought')).toBeInTheDocument()
    // Collapsed by default — same defaultOpen={false} contract as the live
    // stream's Reasoning block; the body only appears after expanding.
    expect(screen.queryByText('Let me think about this')).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: /Thought/ }))
    expect(screen.getByText('Let me think about this')).toBeInTheDocument()
  })

  it('"first" renders on a first-of-turn assistant message', () => {
    setPref('first')
    renderBubble(assistantMessage('First-turn reasoning'), true)
    expect(screen.getByText('Thought')).toBeInTheDocument()
  })

  it('"first" skips a non-first assistant message', () => {
    setPref('first')
    renderBubble(assistantMessage('Later reasoning'), false)
    expect(screen.queryByText('Thought')).not.toBeInTheDocument()
    expect(screen.queryByText('Thinking…')).not.toBeInTheDocument()
    expect(screen.queryByText('Later reasoning')).not.toBeInTheDocument()
  })

  it('"none" hides the thinking block entirely', () => {
    setPref('none')
    renderBubble(assistantMessage('Hidden reasoning'), true)
    expect(screen.queryByText('Thinking…')).not.toBeInTheDocument()
    expect(screen.queryByText('Hidden reasoning')).not.toBeInTheDocument()
  })

  it('renders no thinking block when the message carries none', () => {
    setPref(null)
    renderBubble(assistantMessage(undefined), true)
    expect(screen.queryByText('Thought')).not.toBeInTheDocument()
  })

  it('never renders thinking on a tool message even with the field set', () => {
    setPref(null)
    const toolMessage: ChatMessage = { ...assistantMessage('tool reasoning'), role: 'tool' }
    renderBubble(toolMessage, true)
    expect(screen.queryByText('Thought')).not.toBeInTheDocument()
  })
})

// R3 §4 (Reasoning 头分态) — the committed block's header is the COMPLETED
// state: 「已思考」/「Thought」, with a duration ONLY when the reasoning
// block's timestamps are derivable. They never are on today's wire
// (ChatMessage.thinking is a bare string; QUERY_THINKING carries no ts), so
// the duration-less fallback is the honest render — never an invented 0.
describe('MessageBubble Reasoning header two-state (R3 §4)', () => {
  beforeEach(() => {
    window.localStorage.clear()
  })

  it('renders the completed 「Thought」 header, never the live 「Thinking…」 label', () => {
    setPref(null)
    renderBubble(assistantMessage('Settled reasoning'))
    expect(screen.getByText('Thought')).toBeInTheDocument()
    expect(screen.queryByText('Thinking…')).not.toBeInTheDocument()
  })

  it('renders no fabricated duration when no block timestamps exist on the wire', () => {
    setPref(null)
    renderBubble(assistantMessage('No ts block'))
    expect(screen.getByText('Thought')).not.toHaveTextContent('second')
  })
})

describe('reasoningDurationValue (pure derivation — lights up when a ts field lands)', () => {
  it('returns null without block timestamps — the 「已思考」 fallback case', () => {
    expect(reasoningDurationValue()).toBeNull()
    expect(reasoningDurationValue(null, null)).toBeNull()
    expect(reasoningDurationValue(1_000, null)).toBeNull()
    expect(reasoningDurationValue(null, 1_000)).toBeNull()
  })

  it('derives rounded whole seconds from a valid window', () => {
    expect(reasoningDurationValue(1_000, 13_400)).toEqual({ seconds: 12 })
    expect(reasoningDurationValue(0, 1_000)).toEqual({ seconds: 1 })
  })

  it('drops invalid windows (negative, non-finite, sub-second) instead of a 0', () => {
    expect(reasoningDurationValue(5_000, 1_000)).toBeNull()
    expect(reasoningDurationValue(Number.NaN, 5_000)).toBeNull()
    expect(reasoningDurationValue(1_000, Number.POSITIVE_INFINITY)).toBeNull()
    expect(reasoningDurationValue(1_000, 1_400)).toBeNull()
  })
})

// T1 (R3 §4.1 Reasoning 时长) — the landed turn-level projection's consumers.
describe('reasoningDurationFromMs + matchTurnReasoning (T1 turn projection)', () => {
  const TS = 1_700_000_000_000

  it('reasoningDurationFromMs derives rounded whole seconds from the projected ms', () => {
    expect(reasoningDurationFromMs(42_000)).toEqual({ seconds: 42 })
    expect(reasoningDurationFromMs(500)).toEqual({ seconds: 1 })
  })

  it('reasoningDurationFromMs refuses absent/invalid/sub-second durations (never a 0)', () => {
    expect(reasoningDurationFromMs(undefined)).toBeNull()
    expect(reasoningDurationFromMs(null)).toBeNull()
    expect(reasoningDurationFromMs(0)).toBeNull()
    expect(reasoningDurationFromMs(-4_000)).toBeNull()
    expect(reasoningDurationFromMs(Number.NaN)).toBeNull()
    expect(reasoningDurationFromMs(Number.POSITIVE_INFINITY)).toBeNull()
    expect(reasoningDurationFromMs(400)).toBeNull()
  })

  it('matchTurnReasoning uses overlap (not containment), first window wins', () => {
    const windows: TurnReasoningWindow[] = [
      { startMs: TS - 5_000, endMs: TS + 5_000, reasoningMs: 12_000 },
      { startMs: TS + 60_000, endMs: TS + 90_000, reasoningMs: 30_000 },
    ]
    expect(matchTurnReasoning(windows, TS - 1_000, TS)).toBe(12_000)
    expect(matchTurnReasoning(windows, TS + 70_000, TS + 80_000)).toBe(30_000)
    expect(matchTurnReasoning(windows, TS + 500_000, TS + 600_000)).toBeNull()
    expect(matchTurnReasoning(undefined, TS, TS)).toBeNull()
    expect(matchTurnReasoning(null, TS, TS)).toBeNull()
  })

  it('renders 「Thought for N seconds」 when a reasoning window overlaps the turn', () => {
    setPref(null)
    renderBubble(assistantMessage('Settled reasoning'), false, {
      reasoningWindows: [{ startMs: TS - 1_000, endMs: TS + 40_000, reasoningMs: 42_000 }],
    })
    expect(screen.getByText('Thought for 42 seconds')).toBeInTheDocument()
    expect(screen.queryByText(/^Thought$/)).not.toBeInTheDocument()
  })

  it('keeps the duration-less 「Thought」 when no window overlaps (反例: absent)', () => {
    setPref(null)
    renderBubble(assistantMessage('No projected duration'), false, {
      reasoningWindows: [{ startMs: TS + 1_000_000, endMs: TS + 1_100_000, reasoningMs: 30_000 }],
    })
    expect(screen.getByText('Thought')).toBeInTheDocument()
    expect(screen.queryByText(/second/)).not.toBeInTheDocument()
  })

  it('keeps the duration-less 「Thought」 for a sub-second projected span', () => {
    setPref(null)
    renderBubble(assistantMessage('Blink of thought'), false, {
      reasoningWindows: [{ startMs: TS - 1_000, endMs: TS + 1_000, reasoningMs: 400 }],
    })
    expect(screen.getByText('Thought')).toBeInTheDocument()
    expect(screen.queryByText(/second/)).not.toBeInTheDocument()
  })

  it('keeps the duration-less 「Thought」 when the prop is absent (isolated renders)', () => {
    setPref(null)
    renderBubble(assistantMessage('Plain committed block'))
    expect(screen.getByText('Thought')).toBeInTheDocument()
    expect(screen.queryByText(/second/)).not.toBeInTheDocument()
  })
})
