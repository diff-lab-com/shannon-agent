// R5-2 — in-stream retry notices (failover / key rotation) in AppProvider.
//
// The desktop backend emits `query:notice` when the engine surfaced a
// retry-notice progress line (R3-1 failover, R4-3 key rotation) — the
// request CONTINUED, so the state is informational. These tests drive the
// event through a real AppProvider (only the tauri event module is replaced
// with a capturing fake, same harness as AppContextStreaming.test.tsx) and
// pin the notice lifecycle: per-session bucketing, visible-session
// projection, survival past the run's completion, and clearing on the
// session's next send.

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

describe('AppContext — R5-2 stream notices', () => {
  it('buckets notices per session and projects only the visible one', async () => {
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    await flushUntilRegistered()

    await act(async () => { await result.current.createSession() })
    expect(result.current.currentSessionId).toBe(SESSION_A)
    await act(async () => { await result.current.sendMessage('Hello') })

    // A (visible) fails over; B (background) rotates its key. A's notice
    // carries the id the A-send response returned (A-17: events with a
    // DIFFERENT query id than the session's current one are dropped as late
    // deliveries); B was never sent to from this window, so its events have
    // no recorded id to mismatch — they pass the filter untouched.
    act(() => {
      flush(EVENT_NAMES.QUERY_NOTICE, {
        query_id: 'q1', kind: 'failover',
        message: 'falling back to glm-5.3-flash@zhipu (rate limited)', session_id: SESSION_A,
      })
      flush(EVENT_NAMES.QUERY_NOTICE, {
        query_id: 'q-b', kind: 'key_rotation',
        message: 'rotating API key (1/3) for openai (429)', session_id: SESSION_B,
      })
    })

    // The visible projection shows ONLY session A's notice.
    await waitFor(() => expect(result.current.streamNotices).toHaveLength(1))
    expect(result.current.streamNotices[0].kind).toBe('failover')
    expect(result.current.streamNotices[0].message).toContain('falling back to')

    // Opening B shows B's own bucket — never A's.
    await act(async () => { await result.current.switchSession(SESSION_B) })
    expect(result.current.streamNotices).toHaveLength(1)
    expect(result.current.streamNotices[0].kind).toBe('key_rotation')

    // Back to A: its bucket survived the detour.
    await act(async () => { await result.current.switchSession(SESSION_A) })
    expect(result.current.streamNotices.map(n => n.kind)).toEqual(['failover'])
  })

  it('notices survive the run completing and clear on the next send', async () => {
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    await flushUntilRegistered()

    await act(async () => { await result.current.createSession() })
    await act(async () => { await result.current.sendMessage('Hello') })
    act(() => {
      flush(EVENT_NAMES.QUERY_NOTICE, {
        query_id: 'q1', kind: 'failover',
        message: 'falling back to gpt-5-mini@openai (5xx)', session_id: SESSION_A,
      })
    })
    await waitFor(() => expect(result.current.streamNotices).toHaveLength(1))

    // The run completes: the notice STAYS (the user can still see how the
    // answer was served — the failover succeeded, this is not an error).
    act(() => { flush(EVENT_NAMES.QUERY_COMPLETED, { session_id: SESSION_A }) })
    expect(result.current.streamNotices).toHaveLength(1)

    // The session's next send starts a clean slate.
    await act(async () => { await result.current.sendMessage('Again') })
    expect(result.current.streamNotices).toEqual([])

    // And a notice arriving after the clear is for the new turn only (the
    // api mock answers every send with q1, so the new turn's notice carries
    // the same current id and passes the A-17 filter).
    act(() => {
      flush(EVENT_NAMES.QUERY_NOTICE, {
        query_id: 'q1', kind: 'key_rotation',
        message: 'rotating API key (2/3) for anthropic (401)', session_id: SESSION_A,
      })
    })
    await waitFor(() => expect(result.current.streamNotices).toHaveLength(1))
    expect(result.current.streamNotices[0].kind).toBe('key_rotation')
  })

  it('unknown notice kinds degrade to failover instead of crashing', async () => {
    // Forward-compat: the backend may add kinds (plain retries, …) before
    // the UI knows them; the projection must not break.
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    await flushUntilRegistered()
    await act(async () => { await result.current.createSession() })
    await act(async () => { await result.current.sendMessage('Hello') })
    act(() => {
      flush(EVENT_NAMES.QUERY_NOTICE, {
        query_id: 'q1', kind: 'retry',
        message: 'API retry 1/5 (next try in 2s)', session_id: SESSION_A,
      })
    })
    await waitFor(() => expect(result.current.streamNotices).toHaveLength(1))
    expect(result.current.streamNotices[0].kind).toBe('failover')
  })
})
