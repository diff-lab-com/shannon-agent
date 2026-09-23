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
import { listen } from '@tauri-apps/api/event'
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
    // own bucket (the old single buffer produced "A1B1A2B2").
    expect(result.current.streamingText).toBe('A1A2')

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

    expect(result.current.thinkingText).toBe('think-A more-A')
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
    expect(result.current.streamingText).toBe('A1 ')

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
