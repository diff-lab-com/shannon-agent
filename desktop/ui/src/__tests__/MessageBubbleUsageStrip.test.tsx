// G4b (2026-10-15 parity review §5-G4b 「消息级统计」) — MessageBubble's
// hover usage detail strip + the 「本地模型 · $0」 hint.
//
// Data path under test: the `trace_timeline` IPC (TimelineTurn per-turn
// in/out/cache + wall span) read by hooks/useTurnUsage (module-cached, one
// IPC per session), matched to the bubble's turn window by timestamp
// OVERLAP (matchTurnUsage — the rewind-proof rule matchTurnCost uses).
// Granularity ruling: message-level usage does not exist on the wire — the
// strip renders turn-level figures and titles itself 「本轮/Turn usage」.
//
// Honesty contract under test:
//   no matched turn → no strip · all-zero tokens → no strip ·
//   unknown/absent cost → no ≈$ segment (G3: ≈ only on a real estimate) ·
//   sub-second spans and zero output → no tok/s (never 0, never ∞) ·
//   local hint renders only when usage exists AND the session's serving
//   model/provider is local (ollama).

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { IntlProvider } from 'react-intl'

import {
  MessageBubble,
  matchTurnUsage,
  turnTokensPerSecond,
  turnUsageStats,
  type TurnCostWindow,
} from '@/components/chat/MessageBubble'
import { resetTurnUsageCacheForTests, resolveLocalModel } from '@/hooks/useTurnUsage'
import { ChatContext, type ChatContextValue } from '@/context/ChatContext'
import { SessionContext, type SessionContextValue } from '@/context/SessionContext'
import { CatalogContext, type CatalogContextValue } from '@/context/CatalogContext'
import type { ChatMessage, TimelineTurn, TurnTimeline } from '@/types'
import en from '@/i18n/locales/en.json'

const getTraceTimeline = vi.hoisted(() => vi.fn())

vi.mock('@/lib/tauri-api', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  getTraceTimeline: (...args: unknown[]) => getTraceTimeline(...args),
}))

// G4b i18n: the two new keys land with the 主控's locale batch — the tests
// pin their agreed copy so the strip asserts real words, not raw key ids.
const messages = {
  ...(en as Record<string, string>),
  'chat.message.usageTurn.title': 'Turn usage',
  'chat.message.usageTurn.localFree': 'Local model · $0',
}

const BASE = Date.UTC(2026, 0, 1, 12, 0, 0)

const user = (timestamp: number, overrides: Partial<ChatMessage> = {}): ChatMessage => ({
  role: 'user',
  content: 'question',
  timestamp,
  ...overrides,
})

const assistant = (timestamp: number, overrides: Partial<ChatMessage> = {}): ChatMessage => ({
  role: 'assistant',
  content: 'answer',
  timestamp,
  ...overrides,
})

function fixtureTurn(overrides: Partial<TimelineTurn> = {}): TimelineTurn {
  return {
    turn: 1,
    start_ts_ns: BASE * 1e6,
    end_ts_ns: (BASE + 30_000) * 1e6,
    input_tokens: 4820,
    output_tokens: 1240,
    cache_creation_tokens: 1200,
    cache_read_tokens: 3600,
    tools: [],
    ...overrides,
  }
}

function fixtureTimeline(turns: TimelineTurn[], model: string | null = 'llama3.2'): TurnTimeline {
  return {
    session_id: 's-1',
    model,
    started_ts_ns: BASE * 1e6,
    ended_ts_ns: (BASE + 60_000) * 1e6,
    turns,
    cumulative: [],
  }
}

const COST_WINDOWS: TurnCostWindow[] = [
  { startMs: BASE - 1_000, endMs: BASE + 30_000, costUsd: 0.0214 },
]

const sessionValue = {
  sessions: [],
  sessionActivity: {},
  goalRunsBySession: {},
  subagentLive: null,
  currentSessionId: 's-1',
  windowSessionId: null,
  switchingSession: false,
  switchSession: vi.fn().mockResolvedValue(undefined),
} as unknown as SessionContextValue

function makeChatValue(listMessages: ChatMessage[]): ChatContextValue {
  return {
    messages: listMessages,
    sendMessage: vi.fn().mockResolvedValue(true),
    cancelQuery: vi.fn().mockResolvedValue(undefined),
    isQuerying: false,
    promptQueue: [],
    feedback: {},
    recordFeedback: vi.fn().mockResolvedValue(undefined),
    checkpoints: [],
    rewindSession: vi.fn().mockResolvedValue(undefined),
    compactSession: vi.fn(),
    contextPanelOpen: false,
    toggleContextPanel: vi.fn(),
    setContextPanelOpen: vi.fn(),
  } as unknown as ChatContextValue
}

/** Catalog stub: `models` feed resolveLocalModel's id/name match;
 *  `providerStatus.kind` is the active-provider fallback. */
function catalogValue(models: { id: string; name: string; provider: string }[] = [], providerKind: string | null = null): CatalogContextValue {
  return {
    models: models.map(m => ({ context_window: 0, ...m })),
    providerStatus: { active_provider_id: null, display_name: null, kind: providerKind, has_api_key: false, model: null },
    status: null,
    config: null,
    agents: [],
    tasks: [],
    mcpServers: [],
    backgroundTasks: [],
    permissionRequest: null,
  } as unknown as CatalogContextValue
}

function renderTurn(
  listMessages: ChatMessage[],
  index: number,
  opts: {
    costWindows?: TurnCostWindow[]
    catalog?: CatalogContextValue
  } = {},
) {
  return render(
    <MemoryRouter>
      <ChatContext.Provider value={makeChatValue(listMessages)}>
        <SessionContext.Provider value={sessionValue}>
          <CatalogContext.Provider value={opts.catalog ?? catalogValue()}>
            <IntlProvider locale="en" defaultLocale="en" messages={messages}>
              <MessageBubble
                message={listMessages[index]}
                messageIndex={index}
                onViewDiff={vi.fn()}
                costWindows={opts.costWindows}
              />
            </IntlProvider>
          </CatalogContext.Provider>
        </SessionContext.Provider>
      </ChatContext.Provider>
    </MemoryRouter>,
  )
}

beforeEach(() => {
  getTraceTimeline.mockReset()
  resetTurnUsageCacheForTests()
})

describe('G4b pure derivations', () => {
  it('turnTokensPerSecond — output ÷ span seconds, 1 decimal below 100', () => {
    expect(turnTokensPerSecond(1240, 30_000)).toBe(41.3)
    expect(turnTokensPerSecond(150, 5_000)).toBe(30)
    expect(turnTokensPerSecond(5, 2_000)).toBe(2.5)
  })

  it('turnTokensPerSecond — integers at/above 100 tok/s', () => {
    expect(turnTokensPerSecond(2_500, 10_000)).toBe(250)
  })

  it('turnTokensPerSecond — refuses zero output, missing and sub-second spans', () => {
    expect(turnTokensPerSecond(0, 30_000)).toBeNull()
    expect(turnTokensPerSecond(1240, null)).toBeNull()
    expect(turnTokensPerSecond(1240, 400)).toBeNull()
    expect(turnTokensPerSecond(1240, Number.NaN)).toBeNull()
  })

  it('matchTurnUsage — overlap (not containment), first hit wins, absent → null', () => {
    const turns = [
      fixtureTurn({ turn: 1, start_ts_ns: (BASE + 5_000) * 1e6, end_ts_ns: (BASE + 50_000) * 1e6, output_tokens: 1 }),
      fixtureTurn({ turn: 2, start_ts_ns: (BASE + 60_000) * 1e6, end_ts_ns: (BASE + 90_000) * 1e6, output_tokens: 2 }),
    ]
    expect(matchTurnUsage(turns, BASE, BASE + 10_000)?.turn).toBe(1)
    expect(matchTurnUsage(turns, BASE + 70_000, BASE + 80_000)?.turn).toBe(2)
    expect(matchTurnUsage(turns, BASE + 200_000, BASE + 300_000)).toBeNull()
    expect(matchTurnUsage(undefined, BASE, BASE + 1)).toBeNull()
    expect(matchTurnUsage(null, BASE, BASE + 1)).toBeNull()
  })

  it('turnUsageStats — wall span in ms + derived tok/s; zero-width → nulls', () => {
    const stats = turnUsageStats(fixtureTurn())
    expect(stats.inputTokens).toBe(4820)
    expect(stats.outputTokens).toBe(1240)
    expect(stats.cacheReadTokens).toBe(3600)
    expect(stats.cacheCreationTokens).toBe(1200)
    expect(stats.durationMs).toBe(30_000)
    expect(stats.tokensPerSecond).toBe(41.3)
    const degenerate = turnUsageStats(fixtureTurn({ end_ts_ns: BASE * 1e6 }))
    expect(degenerate.durationMs).toBeNull()
    expect(degenerate.tokensPerSecond).toBeNull()
  })

  it('resolveLocalModel — a known session model decides, kind is the fallback', () => {
    const catalog = [
      { id: 'glm-5.3', name: 'GLM 5.3', provider: 'zhipu' },
      { id: 'llama3.2', name: 'Llama 3.2', provider: 'ollama' },
    ]
    expect(resolveLocalModel('llama3.2', catalog, null)).toBe(true)
    // Session served by a KNOWN cloud model stays non-local even when the
    // user has since switched the active provider to Ollama.
    expect(resolveLocalModel('glm-5.3', catalog, { kind: 'ollama' })).toBe(false)
    // Unknown model id → active provider kind decides.
    expect(resolveLocalModel('mystery-model', catalog, { kind: 'ollama' })).toBe(true)
    expect(resolveLocalModel('mystery-model', catalog, { kind: 'anthropic' })).toBe(false)
    expect(resolveLocalModel(null, catalog, null)).toBe(false)
  })
})

describe('G4b hover usage strip', () => {
  it('renders ↓in · ↑out · ↻cache · duration · tok/s · ≈$ on a matched turn', async () => {
    getTraceTimeline.mockResolvedValue(fixtureTimeline([fixtureTurn()]))
    const list = [user(BASE), assistant(BASE + 25_000)]
    renderTurn(list, 1, { costWindows: COST_WINDOWS })
    const strip = await screen.findByTestId('message-usage-strip')
    expect(strip).toHaveTextContent('↓ 4,820')
    expect(strip).toHaveTextContent('↑ 1,240')
    expect(strip).toHaveTextContent('↻ 3,600/1,200')
    expect(strip).toHaveTextContent('30 seconds')
    expect(strip).toHaveTextContent('41.3 tok/s')
    // G3 estimate convention; sub-dollar figure at the design's 3 decimals.
    expect(strip).toHaveTextContent('≈$0.021')
    // One coherent group label; the title carries the turn-level note.
    expect(strip).toHaveAttribute('aria-label', 'This turn: Input 4,820, output 1,240 tokens · Cache tokens 3,600/1,200 · 30 seconds · 41.3 tok/s · ≈$0.021')
    expect(strip).toHaveAttribute('title', 'Turn usage')
  })

  it('is hover/focus-revealed only (opacity-0 rest state, group-hover/focus-within shown)', async () => {
    getTraceTimeline.mockResolvedValue(fixtureTimeline([fixtureTurn()]))
    const list = [user(BASE), assistant(BASE + 25_000)]
    renderTurn(list, 1)
    const strip = await screen.findByTestId('message-usage-strip')
    expect(strip.className).toContain('opacity-0')
    expect(strip.className).toContain('group-hover:opacity-100')
    expect(strip.className).toContain('group-focus-within:opacity-100')
    expect(strip.className).not.toContain('shadow-')
  })

  it('hides the ≈$ segment when the turn has no cost (unknown rate, G3)', async () => {
    getTraceTimeline.mockResolvedValue(fixtureTimeline([fixtureTurn({ cost_usd: null })]))
    const list = [user(BASE), assistant(BASE + 25_000)]
    renderTurn(list, 1)
    const strip = await screen.findByTestId('message-usage-strip')
    expect(strip).not.toHaveTextContent('$')
  })

  it('appends the success-toned 「Local model · $0」 segment for a local session', async () => {
    getTraceTimeline.mockResolvedValue(fixtureTimeline([fixtureTurn({ cost_usd: null })], 'llama3.2'))
    const list = [user(BASE), assistant(BASE + 25_000)]
    renderTurn(list, 1, {
      catalog: catalogValue([{ id: 'llama3.2', name: 'Llama 3.2', provider: 'ollama' }]),
    })
    const strip = await screen.findByTestId('message-usage-strip')
    const local = strip.querySelector('.text-success')
    expect(local).not.toBeNull()
    expect(local).toHaveTextContent('Local model · $0')
  })

  it('no local hint when the session model resolves to a cloud provider', async () => {
    getTraceTimeline.mockResolvedValue(fixtureTimeline([fixtureTurn()], 'glm-5.3'))
    const list = [user(BASE), assistant(BASE + 25_000)]
    renderTurn(list, 1, {
      catalog: catalogValue(
        [{ id: 'glm-5.3', name: 'GLM 5.3', provider: 'zhipu' }],
        'ollama', // active provider is ollama — the session was NOT
      ),
    })
    const strip = await screen.findByTestId('message-usage-strip')
    expect(strip.querySelector('.text-success')).toBeNull()
  })

  it('renders nothing when no timeline turn overlaps (absent = hidden)', async () => {
    getTraceTimeline.mockResolvedValue(
      fixtureTimeline([fixtureTurn({ start_ts_ns: (BASE + 1_000_000) * 1e6, end_ts_ns: (BASE + 1_030_000) * 1e6 })]),
    )
    const list = [user(BASE), assistant(BASE + 25_000)]
    renderTurn(list, 1)
    await screen.findByTestId('turn-meta-done') // bubble settled
    expect(screen.queryByTestId('message-usage-strip')).toBeNull()
  })

  it('renders nothing when the timeline read fails (opportunistic surface)', async () => {
    getTraceTimeline.mockRejectedValue(new Error('ipc down'))
    const list = [user(BASE), assistant(BASE + 25_000)]
    renderTurn(list, 1)
    await screen.findByTestId('turn-meta-done')
    expect(screen.queryByTestId('message-usage-strip')).toBeNull()
  })

  it('renders nothing when the matched turn carries all-zero tokens', async () => {
    getTraceTimeline.mockResolvedValue(
      fixtureTimeline([fixtureTurn({ input_tokens: 0, output_tokens: 0, cache_read_tokens: 0, cache_creation_tokens: 0 })]),
    )
    const list = [user(BASE), assistant(BASE + 25_000)]
    renderTurn(list, 1)
    await screen.findByTestId('turn-meta-done')
    expect(screen.queryByTestId('message-usage-strip')).toBeNull()
  })

  it('renders nothing on user messages even when a turn overlaps', async () => {
    getTraceTimeline.mockResolvedValue(fixtureTimeline([fixtureTurn()]))
    const list = [user(BASE), assistant(BASE + 25_000)]
    renderTurn(list, 0)
    await screen.findByText('question')
    expect(screen.queryByTestId('message-usage-strip')).toBeNull()
  })

  it('renders nothing on tool-role messages', async () => {
    getTraceTimeline.mockResolvedValue(fixtureTimeline([fixtureTurn()]))
    const list: ChatMessage[] = [
      user(BASE),
      assistant(BASE + 25_000),
      { role: 'tool', content: 'tool ran', timestamp: BASE + 26_000 },
    ]
    renderTurn(list, 2)
    await screen.findByText('tool ran')
    expect(screen.queryByTestId('message-usage-strip')).toBeNull()
  })
})
