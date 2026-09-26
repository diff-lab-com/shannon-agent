// B1 batch — AppContext-level behavior:
//   * P1-5  per-session query state (background runs must not disable the
//           visible session's composer; cancelQuery targets the visible run)
//   * P2-13 throttled streaming projection (coalescing + final integrity)
//   * P1-13 contextPanelOpen persistence (shannon.dock.open)
//   * §4-9  prompt queue (cap / FIFO / remove / drain take)
//
// Harness mirrors AppContextStreaming.test.tsx: a real AppProvider whose
// only replacement is a capturing @tauri-apps/api/event fake.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { renderHook, act, waitFor } from '@testing-library/react'
import { AppProvider, useApp } from '@/context/AppContext'
import { EVENT_NAMES } from '@/types'
import * as api from '@/lib/tauri-api'

const SESSION_A = 'aaaa1111-0000-4000-8000-00000000000a'
const SESSION_B = 'bbbb2222-0000-4000-8000-00000000000b'

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
  localStorage.clear()
  for (const key of Object.keys(captured)) delete captured[key]
  vi.mocked(api.listSessions).mockResolvedValue([])
  vi.mocked(api.newSession).mockResolvedValue(SESSION_A)
  vi.mocked(api.sendMessage).mockResolvedValue({ query_id: 'q1' })
  vi.mocked(api.switchSession).mockResolvedValue([])
})

describe('B1 P1-5 — isQuerying is per session', () => {
  it('a background run does not disable the foreground composer', async () => {
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    await flushUntilRegistered()

    await act(async () => { await result.current.createSession() })
    expect(result.current.currentSessionId).toBe(SESSION_A)

    // Start a run on session B, then go back to A: B keeps streaming in the
    // background while A is on screen.
    await act(async () => { await result.current.switchSession(SESSION_B) })
    await act(async () => { await result.current.sendMessage('bg prompt') })
    act(() => { flush(EVENT_NAMES.QUERY_TEXT, { content: 'B1', session_id: SESSION_B }) })
    expect(result.current.isQuerying).toBe(true) // B is the visible session here

    await act(async () => { await result.current.switchSession(SESSION_A) })
    // B is still running — but A is visible, so ITS composer stays free.
    expect(result.current.isQuerying).toBe(false)
    act(() => { flush(EVENT_NAMES.QUERY_TEXT, { content: 'B2', session_id: SESSION_B }) })
    expect(result.current.isQuerying).toBe(false)

    // When A itself starts, the composer gates on ITS OWN run.
    await act(async () => { await result.current.sendMessage('hello') })
    expect(result.current.isQuerying).toBe(true)

    // B completing in the background must not settle A's composer state.
    act(() => { flush(EVENT_NAMES.QUERY_COMPLETED, { session_id: SESSION_B }) })
    expect(result.current.isQuerying).toBe(true)

    act(() => { flush(EVENT_NAMES.QUERY_COMPLETED, { session_id: SESSION_A }) })
    expect(result.current.isQuerying).toBe(false)
  })

  it('a completed background run leaves the visible session querying', async () => {
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    await flushUntilRegistered()

    await act(async () => { await result.current.createSession() })
    await act(async () => { await result.current.sendMessage('visible run') })
    expect(result.current.isQuerying).toBe(true)

    // B (background) finishing must not settle A's composer state.
    act(() => { flush(EVENT_NAMES.QUERY_COMPLETED, { session_id: SESSION_B }) })
    expect(result.current.isQuerying).toBe(true)

    act(() => { flush(EVENT_NAMES.QUERY_COMPLETED, { session_id: SESSION_A }) })
    expect(result.current.isQuerying).toBe(false)
  })

  it('a failed background run does not surface its error in the foreground', async () => {
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    await flushUntilRegistered()

    await act(async () => { await result.current.createSession() })
    await act(async () => { await result.current.sendMessage('visible run') })

    act(() => { flush(EVENT_NAMES.QUERY_FAILED, { error: 'boom-bg', session_id: SESSION_B }) })
    expect(result.current.error).toBeNull()
    expect(result.current.isQuerying).toBe(true)

    act(() => { flush(EVENT_NAMES.QUERY_FAILED, { error: 'boom-visible', session_id: SESSION_A }) })
    expect(result.current.error).toBe('boom-visible')
    expect(result.current.isQuerying).toBe(false)
  })

  it('cancelQuery targets the visible session', async () => {
    const cancelSpy = vi.spyOn(api, 'cancelQuery').mockResolvedValue(undefined)
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    await flushUntilRegistered()

    await act(async () => { await result.current.createSession() })
    await act(async () => { await result.current.sendMessage('run in A') })
    await act(async () => { await result.current.cancelQuery() })
    expect(cancelSpy).toHaveBeenLastCalledWith(SESSION_A)
    cancelSpy.mockRestore()
  })
})

describe('B1 P2-13 — throttled streaming projection', () => {
  it('coalesces rapid tokens into one flush and never loses the tail', async () => {
    vi.useFakeTimers()
    try {
      const { result } = renderHook(() => useApp(), { wrapper })
      // Drain microtasks (all initial loads are resolved promises) without
      // real timers — RTL waitFor would stall under fake timers here.
      const spin = async () => {
        for (let i = 0; i < 8; i++) await act(async () => { await Promise.resolve() })
      }
      await spin()
      expect(result.current.loading).toBe(false)
      expect((captured[EVENT_NAMES.QUERY_TEXT] ?? []).length).toBeGreaterThan(0)

      await act(async () => { await result.current.createSession() })
      await act(async () => { await result.current.sendMessage('Hello') })

      // Tokens land in the bucket; nothing is projected before the flush
      // window elapses.
      act(() => {
        flush(EVENT_NAMES.QUERY_TEXT, { content: 'a', session_id: SESSION_A })
        flush(EVENT_NAMES.QUERY_TEXT, { content: 'b', session_id: SESSION_A })
        flush(EVENT_NAMES.QUERY_TEXT, { content: 'c', session_id: SESSION_A })
      })
      expect(result.current.streamingText).toBe('')

      // Half the window: still coalescing.
      await act(async () => { await vi.advanceTimersByTimeAsync(20) })
      expect(result.current.streamingText).toBe('')

      // Window elapsed: exactly one flush with ALL buffered tokens.
      await act(async () => { await vi.advanceTimersByTimeAsync(30) })
      expect(result.current.streamingText).toBe('abc')

      // Tail tokens + an immediate COMPLETED: the final commit reads the
      // bucket, so no tail can be lost to a pending flush.
      act(() => {
        flush(EVENT_NAMES.QUERY_TEXT, { content: 'def', session_id: SESSION_A })
        flush(EVENT_NAMES.QUERY_COMPLETED, { session_id: SESSION_A })
      })
      expect(result.current.streamingText).toBe('')
      const assistants = result.current.messages.filter(m => m.role === 'assistant')
      expect(assistants).toHaveLength(1)
      expect(assistants[0].content).toBe('abcdef')
    } finally {
      vi.useRealTimers()
    }
  })
})

describe('B1 P1-13 — contextPanelOpen persistence', () => {
  it('restores the persisted dock state and persists every toggle', async () => {
    localStorage.setItem('shannon.dock.open', '1')
    const first = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(first.result.current.loading).toBe(false))
    expect(first.result.current.contextPanelOpen).toBe(true)

    act(() => { first.result.current.toggleContextPanel() })
    expect(first.result.current.contextPanelOpen).toBe(false)
    expect(localStorage.getItem('shannon.dock.open')).toBe('0')

    act(() => { first.result.current.setContextPanelOpen(true) })
    expect(localStorage.getItem('shannon.dock.open')).toBe('1')
    first.unmount()

    // A fresh provider re-reads the persisted value.
    localStorage.setItem('shannon.dock.open', '0')
    const second = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(second.result.current.loading).toBe(false))
    expect(second.result.current.contextPanelOpen).toBe(false)
  })
})

describe('B1 §4-9 — prompt queue primitives', () => {
  it('enqueues FIFO, caps at 3 (overflow rejected), removes by id', async () => {
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    await flushUntilRegistered()
    await act(async () => { await result.current.createSession() })

    const results: boolean[] = []
    await act(async () => {
      results.push(result.current.enqueuePrompt('one', []))
      results.push(result.current.enqueuePrompt('two', ['f1']))
      results.push(result.current.enqueuePrompt('three', []))
      results.push(result.current.enqueuePrompt('four', []))
    })
    expect(results).toEqual([true, true, true, false])
    expect(result.current.promptQueue.map(q => q.text)).toEqual(['one', 'two', 'three'])

    // FIFO drain takes the head.
    let head: { id: number; text: string } | null = null
    await act(async () => {
      head = result.current.dequeuePrompt()
    })
    expect(head?.text).toBe('one')
    expect(result.current.promptQueue.map(q => q.text)).toEqual(['two', 'three'])

    // Remove by id.
    const twoId = result.current.promptQueue[0].id
    await act(async () => { result.current.removeQueuedPrompt(twoId) })
    expect(result.current.promptQueue.map(q => q.text)).toEqual(['three'])

    // The cap is per session: another session's queue is independent.
    await act(async () => { await result.current.switchSession(SESSION_B) })
    await act(async () => {
      expect(result.current.enqueuePrompt('b-one', [])).toBe(true)
    })
    expect(result.current.promptQueue.map(q => q.text)).toEqual(['b-one'])
  })
})
