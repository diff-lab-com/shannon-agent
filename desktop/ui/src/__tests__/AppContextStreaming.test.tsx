// Review §P2-18 — per-session streaming buckets in AppProvider.
//
// The main window receives every session's query:* events, so the old
// single streaming buffer interleaved tokens of concurrent runs and
// committed the mixture as one assistant message. These tests drive two
// sessions' events through a real AppProvider (only the tauri event module
// is replaced with a capturing fake) and assert each consumer sees its own
// session's buffer.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { renderHook, render, screen, act, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { AppProvider, useApp } from '@/context/AppContext'
import { useSessions } from '@/context/SessionContext'
import { SessionsSection } from '@/components/SidebarSessions'
import { I18nProvider } from '@/i18n'
import { EVENT_NAMES } from '@/types'
import * as api from '@/lib/tauri-api'

const SESSION_A = 'aaaa1111-0000-4000-8000-00000000000a'
const SESSION_B = 'bbbb2222-0000-4000-8000-00000000000b'

// Capture every listen() registration so tests can drive payloads.
const { captured, flush } = vi.hoisted(() => {
  const captured: Record<string, ((e: { payload: unknown }) => void)[]> = {}
  const flush = (event: string, payload: unknown) => {
    for (const h of captured[event] ?? []) h(payload)
  }
  return { captured, flush }
})

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn((event: string, handler: (e: { payload: unknown }) => void) => {
    captured[event] ??= []
    captured[event].push((payload) => handler({ payload }))
    return Promise.resolve(() => {
      captured[event] = (captured[event] ?? []).filter(h => h !== handler)
    })
  }),
  emit: vi.fn(),
}))

function wrapper({ children }: { children: React.ReactNode }) {
  return <AppProvider>{children}</AppProvider>
}

async function flushUntilRegistered() {
  await waitFor(() => {
    expect((captured[EVENT_NAMES.QUERY_TEXT] ?? []).length).toBeGreaterThan(0)
  })
}

beforeEach(() => {
  vi.clearAllMocks()
  for (const key of Object.keys(captured)) delete captured[key]
  vi.mocked(api.listSessions).mockResolvedValue([])
  vi.mocked(api.newSession).mockResolvedValue(SESSION_A)
  vi.mocked(api.sendMessage).mockResolvedValue({ query_id: 'q1' })
  vi.mocked(api.switchSession).mockResolvedValue([])
})

describe('AppContext — §P2-18 per-session streaming buckets', () => {
  it('never mixes tokens of two concurrently streaming sessions', async () => {
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    await flushUntilRegistered()

    // Visible session A; B streams in the background (main window receives
    // every session's events).
    await act(async () => { await result.current.createSession() })
    expect(result.current.currentSessionId).toBe(SESSION_A)
    await act(async () => { await result.current.sendMessage('Hello') })
    expect(result.current.isQuerying).toBe(true)

    act(() => {
      flush(EVENT_NAMES.QUERY_TEXT, { content: 'A1', session_id: SESSION_A })
      flush(EVENT_NAMES.QUERY_TEXT, { content: 'B1', session_id: SESSION_B })
      flush(EVENT_NAMES.QUERY_TEXT, { content: 'A2', session_id: SESSION_A })
      flush(EVENT_NAMES.QUERY_TEXT, { content: 'B2', session_id: SESSION_B })
    })

    // The visible stream shows ONLY session A's tokens — B's went to B's
    // own bucket (the old single buffer produced "A1B1A2B2"). B1 P2-13:
    // the visible projection is throttled (~50ms), so await the flush.
    await waitFor(() => expect(result.current.streamingText).toBe('A1A2'))

    // B completing must not append its (own) text to the visible session's
    // messages, clear the visible stream, or settle the composer state of
    // the still-streaming session A.
    act(() => { flush(EVENT_NAMES.QUERY_COMPLETED, { session_id: SESSION_B }) })
    expect(result.current.streamingText).toBe('A1A2')
    expect(result.current.isQuerying).toBe(true)
    expect(result.current.messages.filter(m => m.role === 'assistant')).toHaveLength(0)

    // A completing commits A's own bucket — exactly once, unmixed.
    act(() => { flush(EVENT_NAMES.QUERY_COMPLETED, { session_id: SESSION_A }) })
    expect(result.current.streamingText).toBe('')
    expect(result.current.isQuerying).toBe(false)
    const assistants = result.current.messages.filter(m => m.role === 'assistant')
    expect(assistants).toHaveLength(1)
    expect(assistants[0].content).toBe('A1A2')
  })

  it('thinking text is bucketed the same way', async () => {
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    await flushUntilRegistered()

    await act(async () => { await result.current.createSession() })
    await act(async () => { await result.current.sendMessage('Hello') })

    act(() => {
      flush(EVENT_NAMES.QUERY_THINKING, { content: 'think-A ', session_id: SESSION_A })
      flush(EVENT_NAMES.QUERY_THINKING, { content: 'think-B ', session_id: SESSION_B })
      flush(EVENT_NAMES.QUERY_THINKING, { content: 'more-A', session_id: SESSION_A })
    })

    // B1 P2-13: thinking projections ride the same throttled flush.
    await waitFor(() => expect(result.current.thinkingText).toBe('think-A more-A'))
  })

  it('switching to a background session shows that session\'s own bucket', async () => {
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    await flushUntilRegistered()

    await act(async () => { await result.current.createSession() })
    await act(async () => { await result.current.sendMessage('Hello') })

    act(() => {
      flush(EVENT_NAMES.QUERY_TEXT, { content: 'A1 ', session_id: SESSION_A })
      flush(EVENT_NAMES.QUERY_TEXT, { content: 'B1', session_id: SESSION_B })
      flush(EVENT_NAMES.QUERY_THINKING, { content: 'thought-B', session_id: SESSION_B })
    })
    await waitFor(() => expect(result.current.streamingText).toBe('A1 '))

    // Open the background session: its own buffered text is projected.
    await act(async () => { await result.current.switchSession(SESSION_B) })
    expect(result.current.currentSessionId).toBe(SESSION_B)
    expect(result.current.streamingText).toBe('B1')
    expect(result.current.thinkingText).toBe('thought-B')

    // A keeps streaming in the background — the B view is untouched.
    act(() => { flush(EVENT_NAMES.QUERY_TEXT, { content: 'A2', session_id: SESSION_A }) })
    expect(result.current.streamingText).toBe('B1')

    // Back to A: its bucket continued accumulating while unseen.
    await act(async () => { await result.current.switchSession(SESSION_A) })
    expect(result.current.streamingText).toBe('A1 A2')
  })
})

// G3 P0-3 — refusals ride along with a SUCCESSFUL send (partial success):
// each one must produce a "«file» was not sent: «reason»" toast without
// failing or rolling back the message.
describe('AppContext — P0-3 rejected-attachment toasts', () => {
  it('toasts per refused attachment from the sendMessage response', async () => {
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    await flushUntilRegistered()
    await act(async () => { await result.current.createSession() })

    vi.mocked(api.sendMessage).mockResolvedValue({
      query_id: 'q1',
      rejected_attachments: [
        { path: '/home/u/Downloads/report.pdf', reason: 'out_of_working_dir' },
      ],
    })

    const { toast } = await import('sonner')
    const warningSpy = vi.spyOn(toast, 'warning').mockImplementation(() => 'x')
    await act(async () => { await result.current.sendMessage('see attached', ['/home/u/Downloads/report.pdf']) })

    expect(warningSpy).toHaveBeenCalledWith(
      'report.pdf was not sent: it is outside the working directory Shannon may read',
    )
    // Partial success: the send stands (no rollback of the user message).
    expect(result.current.messages.some(m => m.role === 'user' && m.content === 'see attached')).toBe(true)
    expect(result.current.error).toBeNull()
    warningSpy.mockRestore()
  })
})

// B0 P1-2 — a failed/cancelled run leaves no ghost streaming bubble: the
// run's buckets are dropped and, for the visible session, the projections
// (streamingText / thinkingText / activeToolCalls) reset along with
// isQuerying. Persisting the partial text needs a backend commit path, so
// clearing is the approved behavior.
describe('AppContext — B0 P1-2 ghost-bubble cleanup on fail/cancel', () => {
  async function setupStreamingSession() {
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    await flushUntilRegistered()
    await act(async () => { await result.current.createSession() })
    await act(async () => { await result.current.sendMessage('Hello') })
    act(() => {
      flush(EVENT_NAMES.QUERY_TEXT, { content: 'partial answer', session_id: SESSION_A })
      flush(EVENT_NAMES.QUERY_THINKING, { content: 'partial thought ', session_id: SESSION_A })
      flush(EVENT_NAMES.QUERY_TOOL_START, {
        tool_use_id: 'tc-1', tool_name: 'bash', tool_input: {}, session_id: SESSION_A,
      })
    })
    await waitFor(() => {
      expect(result.current.streamingText).toBe('partial answer')
      expect(result.current.thinkingText).toBe('partial thought ')
    })
    expect(result.current.activeToolCalls).toHaveLength(1)
    expect(result.current.isQuerying).toBe(true)
    return result
  }

  it('QUERY_FAILED clears streaming/thinking/tool calls and the buckets', async () => {
    const result = await setupStreamingSession()

    act(() => { flush(EVENT_NAMES.QUERY_FAILED, { error: 'engine exploded', session_id: SESSION_A }) })

    expect(result.current.error).toBe('engine exploded')
    expect(result.current.isQuerying).toBe(false)
    expect(result.current.streamingText).toBe('')
    expect(result.current.thinkingText).toBe('')
    expect(result.current.activeToolCalls).toHaveLength(0)
    // No ghost assistant bubble is committed, and the failure did not leave
    // residue in the session's bucket (switching away and back stays clean).
    expect(result.current.messages.filter(m => m.role === 'assistant')).toHaveLength(0)
    await act(async () => { await result.current.switchSession(SESSION_B) })
    await act(async () => { await result.current.switchSession(SESSION_A) })
    expect(result.current.streamingText).toBe('')
    expect(result.current.thinkingText).toBe('')
  })

  it('QUERY_CANCELLED drops the ghost bubble and its bucket too', async () => {
    const result = await setupStreamingSession()

    act(() => { flush(EVENT_NAMES.QUERY_CANCELLED, { session_id: SESSION_A }) })

    expect(result.current.isQuerying).toBe(false)
    expect(result.current.streamingText).toBe('')
    expect(result.current.thinkingText).toBe('')
    expect(result.current.activeToolCalls).toHaveLength(0)
    expect(result.current.messages.filter(m => m.role === 'assistant')).toHaveLength(0)
    await act(async () => { await result.current.switchSession(SESSION_B) })
    await act(async () => { await result.current.switchSession(SESSION_A) })
    expect(result.current.streamingText).toBe('')
    expect(result.current.thinkingText).toBe('')
  })
})

// P2-19 — the dedicated visible-session toolProgress slot feeds the
// RunStatusLine pill. QUERY_TOOL_PROGRESS updates it live for the visible
// session only; it clears on run settle (completed/failed/cancelled), on a
// new send and on session switch — never leaking across sessions or runs.
describe('AppContext — P2-19 tool progress lifecycle', () => {
  async function setupStreamingSession() {
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    await flushUntilRegistered()
    await act(async () => { await result.current.createSession() })
    await act(async () => { await result.current.sendMessage('Hello') })
    act(() => {
      flush(EVENT_NAMES.QUERY_TOOL_START, {
        tool_use_id: 'tc-1', tool_name: 'bash', tool_input: {}, session_id: SESSION_A,
      })
    })
    expect(result.current.isQuerying).toBe(true)
    expect(result.current.toolProgress).toBeNull()
    return result
  }

  it('QUERY_TOOL_PROGRESS sets and updates the visible progress', async () => {
    const result = await setupStreamingSession()

    // The backend sends a 0..=1 fraction (agent_loop.rs); the context
    // normalizes it to the 0..=100 percent the pill renders.
    act(() => {
      flush(EVENT_NAMES.QUERY_TOOL_PROGRESS, { tool_use_id: 'tc-1', progress: 0.3, message: 'Compiling', session_id: SESSION_A })
    })
    expect(result.current.toolProgress).toEqual({ progress: 30, message: 'Compiling' })

    act(() => {
      flush(EVENT_NAMES.QUERY_TOOL_PROGRESS, { tool_use_id: 'tc-1', progress: 0.62, message: 'Linking', session_id: SESSION_A })
    })
    expect(result.current.toolProgress).toEqual({ progress: 62, message: 'Linking' })
  })

  it('indeterminate (−1) and out-of-scale progress drops the percentage but keeps the message', async () => {
    const result = await setupStreamingSession()

    act(() => {
      flush(EVENT_NAMES.QUERY_TOOL_PROGRESS, { tool_use_id: 'tc-1', progress: -1, message: 'streaming output…', session_id: SESSION_A })
    })
    expect(result.current.toolProgress).toEqual({ progress: undefined, message: 'streaming output…' })

    act(() => {
      flush(EVENT_NAMES.QUERY_TOOL_PROGRESS, { tool_use_id: 'tc-1', progress: 45, message: 'scale mismatch', session_id: SESSION_A })
    })
    expect(result.current.toolProgress).toEqual({ progress: undefined, message: 'scale mismatch' })
  })

  it('progress from a background session never reaches the visible state', async () => {
    const result = await setupStreamingSession()

    act(() => {
      flush(EVENT_NAMES.QUERY_TOOL_PROGRESS, { tool_use_id: 'tc-bg', progress: 0.99, message: 'background', session_id: SESSION_B })
    })
    expect(result.current.toolProgress).toBeNull()
  })

  it('clears on QUERY_COMPLETED', async () => {
    const result = await setupStreamingSession()
    act(() => {
      flush(EVENT_NAMES.QUERY_TOOL_PROGRESS, { tool_use_id: 'tc-1', progress: 0.45, message: 'halfway', session_id: SESSION_A })
    })
    expect(result.current.toolProgress).toEqual({ progress: 45, message: 'halfway' })

    // Async act: draining microtasks also absorbs the post-settle
    // checkpoints/status refreshes the context fires when isQuerying falls.
    await act(async () => { flush(EVENT_NAMES.QUERY_COMPLETED, { session_id: SESSION_A }) })
    expect(result.current.toolProgress).toBeNull()
    expect(result.current.isQuerying).toBe(false)
  })

  it('clears on QUERY_FAILED', async () => {
    const result = await setupStreamingSession()
    act(() => {
      flush(EVENT_NAMES.QUERY_TOOL_PROGRESS, { tool_use_id: 'tc-1', progress: 0.1, message: 'soon broken', session_id: SESSION_A })
    })

    await act(async () => { flush(EVENT_NAMES.QUERY_FAILED, { error: 'boom', session_id: SESSION_A }) })
    expect(result.current.toolProgress).toBeNull()
  })

  it('clears on QUERY_CANCELLED', async () => {
    const result = await setupStreamingSession()
    act(() => {
      flush(EVENT_NAMES.QUERY_TOOL_PROGRESS, { tool_use_id: 'tc-1', progress: 80, message: 'almost', session_id: SESSION_A })
    })

    await act(async () => { flush(EVENT_NAMES.QUERY_CANCELLED, { session_id: SESSION_A }) })
    expect(result.current.toolProgress).toBeNull()
  })

  it('clears when a new tool starts (no stale % on the next tool)', async () => {
    const result = await setupStreamingSession()
    act(() => {
      flush(EVENT_NAMES.QUERY_TOOL_PROGRESS, { tool_use_id: 'tc-1', progress: 0.45, message: 'halfway', session_id: SESSION_A })
    })
    expect(result.current.toolProgress).toEqual({ progress: 45, message: 'halfway' })

    act(() => {
      flush(EVENT_NAMES.QUERY_TOOL_START, { tool_use_id: 'tc-2', tool_name: 'edit_file', tool_input: {}, session_id: SESSION_A })
    })
    expect(result.current.toolProgress).toBeNull()
  })

  it('clears on a new send', async () => {
    const result = await setupStreamingSession()
    act(() => {
      flush(EVENT_NAMES.QUERY_TOOL_PROGRESS, { tool_use_id: 'tc-1', progress: 0.45, message: 'halfway', session_id: SESSION_A })
    })
    expect(result.current.toolProgress).toEqual({ progress: 45, message: 'halfway' })

    await act(async () => { await result.current.sendMessage('again') })
    expect(result.current.toolProgress).toBeNull()
  })

  it('clears on session switch and does not capture the invisible session afterwards', async () => {
    const result = await setupStreamingSession()
    act(() => {
      flush(EVENT_NAMES.QUERY_TOOL_PROGRESS, { tool_use_id: 'tc-1', progress: 0.45, message: 'halfway', session_id: SESSION_A })
    })
    expect(result.current.toolProgress).toEqual({ progress: 45, message: 'halfway' })

    await act(async () => { await result.current.switchSession(SESSION_B) })
    expect(result.current.toolProgress).toBeNull()

    // A keeps running in the background — its progress must not appear in
    // the B view.
    act(() => {
      flush(EVENT_NAMES.QUERY_TOOL_PROGRESS, { tool_use_id: 'tc-1', progress: 90, message: 'later', session_id: SESSION_A })
    })
    expect(result.current.toolProgress).toBeNull()
  })

  // 2026-09-29 provider review §2-3: the desktop classifies QUERY_FAILED
  // payloads Rust-side (`error_kind: "auth" | "other"`). The provider
  // forwards the class so the chat area can route 401/403 to the dedicated
  // update-key banner while every other failure keeps the raw line.
  describe('QUERY_FAILED error_kind classification', () => {
    it('marks auth failures with errorKind "auth"', async () => {
      const { result } = renderHook(() => useApp(), { wrapper })
      await waitFor(() => expect(result.current.loading).toBe(false))
      await flushUntilRegistered()
      await act(async () => { await result.current.createSession() })
      await act(async () => { await result.current.sendMessage('Hello') })

      act(() => {
        flush(EVENT_NAMES.QUERY_FAILED, {
          error: 'Authentication failed',
          error_kind: 'auth',
          session_id: SESSION_A,
        })
      })
      expect(result.current.error).toBe('Authentication failed')
      expect(result.current.errorKind).toBe('auth')
    })

    it('marks every other failure with errorKind "other"', async () => {
      const { result } = renderHook(() => useApp(), { wrapper })
      await waitFor(() => expect(result.current.loading).toBe(false))
      await flushUntilRegistered()
      await act(async () => { await result.current.createSession() })
      await act(async () => { await result.current.sendMessage('Hello') })

      act(() => {
        flush(EVENT_NAMES.QUERY_FAILED, {
          error: 'error sending request',
          error_kind: 'other',
          session_id: SESSION_A,
        })
      })
      expect(result.current.error).toBe('error sending request')
      expect(result.current.errorKind).toBe('other')
    })

    it('a new send clears error and errorKind together', async () => {
      const { result } = renderHook(() => useApp(), { wrapper })
      await waitFor(() => expect(result.current.loading).toBe(false))
      await flushUntilRegistered()
      await act(async () => { await result.current.createSession() })
      await act(async () => { await result.current.sendMessage('Hello') })
      act(() => {
        flush(EVENT_NAMES.QUERY_FAILED, { error: 'Authentication failed', error_kind: 'auth', session_id: SESSION_A })
      })
      expect(result.current.errorKind).toBe('auth')
      await act(async () => { await result.current.sendMessage('Retry') })
      expect(result.current.error).toBeNull()
      expect(result.current.errorKind).toBeNull()
    })
  })
})

// A-7 — the backend can re-emit a tool-start for a card the session already
// tracks (resume/replay paths). The old unconditional append produced a
// duplicate card per re-send; a known tool_use_id must keep the existing
// card (first start wins — the result event resolves the shared id either
// way), while a genuinely new id still gets its own card.
describe('AppContext — A-7 tool-start dedup by tool_use_id', () => {
  it('a re-sent tool-start for a known card does not duplicate it', async () => {
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    await flushUntilRegistered()
    await act(async () => { await result.current.createSession() })
    await act(async () => { await result.current.sendMessage('Hello') })

    const start = { tool_use_id: 'tc-dup', tool_name: 'bash', tool_input: { cmd: 1 }, session_id: SESSION_A }
    act(() => { flush(EVENT_NAMES.QUERY_TOOL_START, start) })
    expect(result.current.activeToolCalls).toHaveLength(1)

    // Re-emissions of the same id — even with drifted payloads — must not
    // append a second card nor rewrite the one already running.
    act(() => {
      flush(EVENT_NAMES.QUERY_TOOL_START, { ...start, tool_input: { cmd: 2 } })
      flush(EVENT_NAMES.QUERY_TOOL_START, { ...start, tool_name: 'edit_file' })
    })

    expect(result.current.activeToolCalls).toHaveLength(1)
    expect(result.current.activeToolCalls[0]).toMatchObject({
      tool_use_id: 'tc-dup',
      tool_name: 'bash',
      tool_input: { cmd: 1 },
      status: 'running',
    })
  })

  it('a genuinely new tool id still appends its own card', async () => {
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    await flushUntilRegistered()
    await act(async () => { await result.current.createSession() })
    await act(async () => { await result.current.sendMessage('Hello') })

    act(() => {
      flush(EVENT_NAMES.QUERY_TOOL_START, { tool_use_id: 'tc-a', tool_name: 'bash', tool_input: {}, session_id: SESSION_A })
      flush(EVENT_NAMES.QUERY_TOOL_START, { tool_use_id: 'tc-b', tool_name: 'read_file', tool_input: {}, session_id: SESSION_A })
    })

    expect(result.current.activeToolCalls.map(tc => tc.tool_use_id)).toEqual(['tc-a', 'tc-b'])
  })
})

// S-2 — a pure-text run's ONLY events are `event` ticks (text/thinking/
// usage), which used to touch just the ref: the published sessionActivity
// never gained the session, so the rail showed no Running dot for the whole
// run. The state channel must publish the visible flip (first observation,
// settle transitions) while mid-run ticks keep ref-only updates.
describe('AppContext — S-2 pure-text stream drives the sidebar running state', () => {
  it('publishes running:true on the first text event without re-publishing per chunk', async () => {
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    await flushUntilRegistered()
    await act(async () => { await result.current.createSession() })
    expect(result.current.sessionActivity[SESSION_A]).toBeUndefined()

    act(() => { flush(EVENT_NAMES.QUERY_TEXT, { content: 'a', session_id: SESSION_A }) })
    const afterFirst = result.current.sessionActivity[SESSION_A]
    expect(afterFirst?.running).toBe(true)
    expect(afterFirst?.startedAt).not.toBeNull()

    act(() => {
      flush(EVENT_NAMES.QUERY_TEXT, { content: 'b', session_id: SESSION_A })
      flush(EVENT_NAMES.QUERY_THINKING, { content: 'c', session_id: SESSION_A })
      flush(EVENT_NAMES.QUERY_USAGE, { session_id: SESSION_A })
    })
    // Throttle direction: mid-run ticks touch the ref only — the published
    // record keeps its identity, so streaming never re-renders the rail.
    expect(result.current.sessionActivity[SESSION_A]).toBe(afterFirst)

    // Settle direction: completion flips the published state back.
    await act(async () => { flush(EVENT_NAMES.QUERY_COMPLETED, { session_id: SESSION_A }) })
    expect(result.current.sessionActivity[SESSION_A].running).toBe(false)
  })

  it('restarts the elapsed clock when a settled session starts running again', async () => {
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    await flushUntilRegistered()
    await act(async () => { await result.current.createSession() })

    const t0 = 1_000_000
    const nowSpy = vi.spyOn(Date, 'now').mockReturnValue(t0)
    try {
      act(() => { flush(EVENT_NAMES.QUERY_TEXT, { content: 'first run', session_id: SESSION_A }) })
      expect(result.current.sessionActivity[SESSION_A]).toMatchObject({ running: true, startedAt: t0 })

      act(() => { flush(EVENT_NAMES.QUERY_COMPLETED, { session_id: SESSION_A }) })
      expect(result.current.sessionActivity[SESSION_A].running).toBe(false)

      nowSpy.mockReturnValue(t0 + 5_000)
      act(() => { flush(EVENT_NAMES.QUERY_TEXT, { content: 'second run', session_id: SESSION_A }) })
      // The re-run must not inherit the previous run's startedAt — the rail
      // derives the elapsed badge from it.
      expect(result.current.sessionActivity[SESSION_A]).toMatchObject({ running: true, startedAt: t0 + 5_000 })
    } finally {
      nowSpy.mockRestore()
    }
  })

  // Component direction: with the state channel live, the rail row shows
  // its Running marker during a pure-text stream and drops it on settle.
  it('the session rail shows the Running marker for a text-only stream', async () => {
    vi.mocked(api.listSessions).mockResolvedValue([
      { id: SESSION_A, title: 'Text only', created_at: Date.now() - 60_000, message_count: 0 },
    ])
    function RailBridge() {
      const { sessions, sessionActivity, currentSessionId } = useSessions()
      return (
        <SessionsSection
          sessions={sessions}
          sessionActivity={sessionActivity}
          goalRunsBySession={{}}
          currentSessionId={currentSessionId}
          switchSession={async () => {}}
          renameSession={async () => {}}
          deleteSession={async () => {}}
        />
      )
    }
    render(
      <I18nProvider>
        <AppProvider>
          <MemoryRouter>
            <RailBridge />
          </MemoryRouter>
        </AppProvider>
      </I18nProvider>,
    )
    await waitFor(() => expect(screen.getByTestId(`desktop-session-row-${SESSION_A}`)).toBeInTheDocument())
    // No run yet — no Running marker on the row.
    expect(screen.queryByRole('img', { name: 'Running' })).not.toBeInTheDocument()

    await flushUntilRegistered()
    act(() => { flush(EVENT_NAMES.QUERY_TEXT, { content: 'streaming…', session_id: SESSION_A }) })
    await waitFor(() => expect(screen.getByRole('img', { name: 'Running' })).toBeInTheDocument())

    act(() => { flush(EVENT_NAMES.QUERY_COMPLETED, { session_id: SESSION_A }) })
    await waitFor(() => expect(screen.queryByRole('img', { name: 'Running' })).not.toBeInTheDocument())
  })
})
