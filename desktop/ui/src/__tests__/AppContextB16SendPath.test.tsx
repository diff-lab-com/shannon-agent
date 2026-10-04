// B1-6 P1-7 — the per-session query latch survives a rejected concurrent send.
//
// The backend lock is per session (commands.rs concurrent-query guard): two
// windows sending into the SAME session get one winner (run streams on) and
// one loser (IPC rejected before any run). The loser's catch used to clear
// the latch unconditionally — the UI showed idle while the winner's run was
// still streaming. Both arms of the fix live here:
//   * a send that FOUND the latch on (another run owns it) leaves it on;
//   * a send that took the latch itself and was rejected still clears it.
//
// Harness mirrors AppContextB1.test.tsx: a real AppProvider whose only
// replacement is a capturing @tauri-apps/api/event fake.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { renderHook, act, waitFor } from '@testing-library/react'
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
  vi.mocked(api.switchSession).mockResolvedValue([])
})

describe('B1-6 P1-7 — rejected send vs the per-session latch', () => {
  it('a rejection does NOT clear a latch another send owns (the double-window loser)', async () => {
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    await flushUntilRegistered()
    await act(async () => { await result.current.createSession() })

    // Winner's IPC never settles inside the test — its run streams on.
    const sendSpy = vi.spyOn(api, 'sendMessage')
      .mockImplementationOnce(() => new Promise<boolean>(() => {}))
      .mockRejectedValueOnce(new Error('another query is already running for this session'))

    let winner!: Promise<boolean>
    await act(async () => { winner = result.current.sendMessage('winner') })
    expect(result.current.isQuerying).toBe(true)

    // The loser targets the SAME session once the winner's latch is
    // committed (the cross-window race has exactly this shape).
    let loser!: Promise<boolean>
    await act(async () => { loser = result.current.sendMessage('loser') })
    await act(async () => { await loser })
    expect(await loser).toBe(false)
    // P1-7: the loser's catch must leave the winner's latch alone.
    expect(result.current.isQuerying).toBe(true)

    // The winner's run still settles through its own query events — the
    // composer is not wedged forever, it just stays honest until the run ends.
    act(() => { flush(EVENT_NAMES.QUERY_COMPLETED, { session_id: SESSION_A }) })
    expect(result.current.isQuerying).toBe(false)

    sendSpy.mockRestore()
    void winner // never settles by design; no unhandled rejection exists
  })

  it('a rejection still clears the latch the rejected send took itself', async () => {
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    await flushUntilRegistered()
    await act(async () => { await result.current.createSession() })

    // No run owns the latch when the send fires — the rejection is this
    // send's own failure, so the latch it took must come back off.
    const sendSpy = vi.spyOn(api, 'sendMessage').mockRejectedValue(
      new Error('Session budget exceeded: spent $1.0000 of $1.0000 — continue (ignore once), raise the budget, or stop'),
    )
    let ok!: boolean
    await act(async () => { ok = await result.current.sendMessage('only send') })

    expect(ok).toBe(false)
    expect(result.current.isQuerying).toBe(false)
    sendSpy.mockRestore()
  })
})
