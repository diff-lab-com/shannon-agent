// B1-4 (P1-3) — the stop settle watchdog.
//
// Backend emits are fire-and-forget: the `query:cancelled` event a stop
// waits for can be lost, and the composer latch + "cancelling" stop button
// used to wedge forever. These tests pin the backstop armed by cancelQuery:
// after 15s it reconciles the still-latched session against backend truth
// (get_session_querying) and, when the backend is already idle, runs the
// exact settle of the QUERY_CANCELLED handler (partial committed as an
// `interrupted` bubble, latch + cancel-in-flight cleared). A still-running
// backend buys one extra round, then the watchdog gives up.
//
// Harness mirrors AppContextB1.test.tsx (real AppProvider + capturing
// event fake); fake timers per the B1 P2-13 pattern (spin microtasks
// instead of RTL waitFor).

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { renderHook, act } from '@testing-library/react'
import { AppProvider, useApp } from '@/context/AppContext'
import { EVENT_NAMES } from '@/types'
import * as api from '@/lib/tauri-api'

const SESSION_A = 'aaaa1111-0000-4000-8000-00000000000a'

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

beforeEach(() => {
  vi.clearAllMocks()
  localStorage.clear()
  for (const key of Object.keys(captured)) delete captured[key]
  vi.mocked(api.listSessions).mockResolvedValue([])
  vi.mocked(api.newSession).mockResolvedValue(SESSION_A)
  vi.mocked(api.sendMessage).mockResolvedValue({ query_id: 'q1' })
  vi.mocked(api.switchSession).mockResolvedValue([])
  vi.mocked(api.cancelQuery).mockResolvedValue(undefined)
  vi.mocked(api.getSessionQuerying).mockResolvedValue(false)
})

// All initial loads are resolved promises — drain microtasks without real
// timers (RTL waitFor would stall under fake timers here).
async function spin(times = 8) {
  for (let i = 0; i < times; i++) await act(async () => { await Promise.resolve() })
}

async function setupRunningSession() {
  const { result } = renderHook(() => useApp(), { wrapper })
  await spin()
  expect(result.current.loading).toBe(false)
  expect((captured[EVENT_NAMES.QUERY_TEXT] ?? []).length).toBeGreaterThan(0)
  await act(async () => { await result.current.createSession() })
  await act(async () => { await result.current.sendMessage('run in A') })
  expect(result.current.isQuerying).toBe(true)
  return result
}

describe('B1-4 — stop settle watchdog', () => {
  it('settles the run when the backend reports idle (lost query:cancelled)', async () => {
    vi.useFakeTimers()
    try {
      const result = await setupRunningSession()

      // Residual stream text — the settle must commit it, not wipe it.
      act(() => { flush(EVENT_NAMES.QUERY_TEXT, { query_id: 'q1', content: 'partial an', session_id: SESSION_A }) })

      await act(async () => { await result.current.cancelQuery() })
      expect(result.current.isCancelInFlight).toBe(true)
      expect(result.current.isQuerying).toBe(true)

      // No terminal event arrives. The watchdog fires, reconciles against
      // an idle backend, and runs the cancelled settle itself.
      await act(async () => { await vi.advanceTimersByTimeAsync(15_000) })
      expect(api.getSessionQuerying).toHaveBeenCalledWith(SESSION_A)
      expect(result.current.isQuerying).toBe(false)
      expect(result.current.isCancelInFlight).toBe(false)

      // D6: the residual is a committed interrupted bubble, same shape the
      // QUERY_CANCELLED handler produces.
      const assistants = result.current.messages.filter(m => m.role === 'assistant')
      expect(assistants).toHaveLength(1)
      expect(assistants[0].content).toBe('partial an')
      expect(assistants[0].interrupted).toBe(true)
    } finally {
      vi.useRealTimers()
    }
  })

  it('re-arms once while the backend is still running, then settles', async () => {
    vi.useFakeTimers()
    try {
      const result = await setupRunningSession()
      vi.mocked(api.getSessionQuerying)
        .mockResolvedValueOnce(true) // first round: teardown still in flight
        .mockResolvedValueOnce(false) // second round: idle

      await act(async () => { await result.current.cancelQuery() })

      await act(async () => { await vi.advanceTimersByTimeAsync(15_000) })
      expect(result.current.isQuerying).toBe(true) // busy — keep waiting

      await act(async () => { await vi.advanceTimersByTimeAsync(15_000) })
      expect(api.getSessionQuerying).toHaveBeenCalledTimes(2)
      expect(result.current.isQuerying).toBe(false)
      expect(result.current.isCancelInFlight).toBe(false)
    } finally {
      vi.useRealTimers()
    }
  })

  it('gives up after the extra round and keeps the status quo (latch stays)', async () => {
    vi.useFakeTimers()
    try {
      const result = await setupRunningSession()
      vi.mocked(api.getSessionQuerying).mockResolvedValue(true)

      await act(async () => { await result.current.cancelQuery() })
      await act(async () => { await vi.advanceTimersByTimeAsync(30_000) }) // both rounds
      expect(api.getSessionQuerying).toHaveBeenCalledTimes(2)
      expect(result.current.isQuerying).toBe(true)

      // A third expiry would fire if the watchdog kept re-arming — it must
      // not (the real terminal event, if it ever comes, still settles).
      await act(async () => { await vi.advanceTimersByTimeAsync(15_000) })
      expect(api.getSessionQuerying).toHaveBeenCalledTimes(2)
      expect(result.current.isQuerying).toBe(true)
    } finally {
      vi.useRealTimers()
    }
  })

  it('a delivered query:cancelled disarms the watchdog (no double settle)', async () => {
    vi.useFakeTimers()
    try {
      const result = await setupRunningSession()
      act(() => { flush(EVENT_NAMES.QUERY_TEXT, { query_id: 'q1', content: 'partial', session_id: SESSION_A }) })

      await act(async () => { await result.current.cancelQuery() })
      act(() => { flush(EVENT_NAMES.QUERY_CANCELLED, { query_id: 'q1', session_id: SESSION_A }) })
      expect(result.current.isQuerying).toBe(false)

      // The watchdog window passes — no reconciliation, no second commit.
      await act(async () => { await vi.advanceTimersByTimeAsync(30_000) })
      expect(api.getSessionQuerying).not.toHaveBeenCalled()
      const assistants = result.current.messages.filter(m => m.role === 'assistant')
      expect(assistants).toHaveLength(1)
      expect(assistants[0].interrupted).toBe(true)
    } finally {
      vi.useRealTimers()
    }
  })

  it('a repeated stop does not stack watchdogs (one reconcile per round)', async () => {
    vi.useFakeTimers()
    try {
      const result = await setupRunningSession()
      vi.mocked(api.getSessionQuerying).mockResolvedValue(false)

      await act(async () => { await result.current.cancelQuery() })
      // A second stop while the first watchdog is armed (cancelInFlight
      // dedupes the UI path; the timer must be single regardless).
      await act(async () => { await result.current.cancelQuery() })

      await act(async () => { await vi.advanceTimersByTimeAsync(15_000) })
      expect(api.getSessionQuerying).toHaveBeenCalledTimes(1)
      expect(result.current.isQuerying).toBe(false)
    } finally {
      vi.useRealTimers()
    }
  })

  it('a failing reconciliation IPC degrades soft (no crash, latch untouched)', async () => {
    vi.useFakeTimers()
    try {
      const result = await setupRunningSession()
      vi.mocked(api.getSessionQuerying).mockRejectedValue(new Error('ipc down'))

      await act(async () => { await result.current.cancelQuery() })
      await act(async () => { await vi.advanceTimersByTimeAsync(15_000) })
      expect(api.getSessionQuerying).toHaveBeenCalledTimes(1)
      // Background-refresh policy: soft failure, last-known state kept.
      expect(result.current.isQuerying).toBe(true)
      expect(result.current.isCancelInFlight).toBe(true)
    } finally {
      vi.useRealTimers()
    }
  })
})
