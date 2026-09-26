// Review §P2-18 — per-session streaming buckets in AppProvider.
//
// The main window receives every session's query:* events, so the old
// single streaming buffer interleaved tokens of concurrent runs and
// committed the mixture as one assistant message. These tests drive two
// sessions' events through a real AppProvider (only the tauri event module
// is replaced with a capturing fake) and assert each consumer sees its own
// session's buffer.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { renderHook, act, waitFor } from '@testing-library/react'
import { AppProvider, useApp } from '@/context/AppContext'
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
})
