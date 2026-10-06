// B4 P2-8 — the budget banners survive a session switch by re-deriving
// their state from the persisted backend budget (get_session_budget +
// get_session_usage), matching the backend's own thresholds (warning at
// >= 80% of the cap, exceeded at >= cap). Events stay the live path.
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { renderHook, waitFor, act } from '@testing-library/react'
import { useBudgetGuard } from '@/hooks/useBudgetGuard'
import { EVENT_NAMES } from '@/types'
import * as api from '@/lib/tauri-api'

// A-12 — capturing listen() fake with an ASYNC unlisten, mirroring the real
// bridge: after a session switch, the OLD registration stays live until the
// unlisten promise resolves, which is exactly the window the stale filter
// used to race through.
const { captured, flush } = vi.hoisted(() => {
  const captured: Record<string, Array<(e: { payload: unknown }) => void>> = {}
  const flush = (event: string, payload: unknown) => {
    for (const h of captured[event] ?? []) h({ payload })
  }
  return { captured, flush }
})

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn((event: string, handler: (e: { payload: unknown }) => void) => {
    captured[event] ??= []
    captured[event].push(handler)
    return Promise.resolve(() => {
      captured[event] = (captured[event] ?? []).filter(h => h !== handler)
    })
  }),
}))

beforeEach(() => {
  vi.clearAllMocks()
  for (const key of Object.keys(captured)) delete captured[key]
  vi.mocked(api.getSessionBudget).mockResolvedValue(null)
  vi.mocked(api.getSessionUsage).mockResolvedValue({
    input_tokens: 0, output_tokens: 0, cache_creation_tokens: 0,
    cache_read_tokens: 0, cost_usd: 0, events: 0,
  } as any)
})

describe('useBudgetGuard — banner re-read on switch (B4 P2-8)', () => {
  it('restores the exceeded banner when switching back to an over-cap session', async () => {
    // Only sess-a is over budget; sess-b reads clean.
    vi.mocked(api.getSessionBudget).mockImplementation(async (sid: string) => (sid === 'sess-a' ? 10 : null))
    vi.mocked(api.getSessionUsage).mockImplementation(async (sid: string) =>
      (sid === 'sess-a' ? { cost_usd: 10.5 } : { cost_usd: 1 }) as any)
    const { result, rerender } = renderHook(({ sid }) => useBudgetGuard(sid), {
      initialProps: { sid: 'sess-a' as string | null },
    })
    await waitFor(() => expect(result.current.exceeded).not.toBeNull())
    expect(result.current.exceeded).toMatchObject({ sessionId: 'sess-a', spentUsd: 10.5, budgetUsd: 10 })
    expect(result.current.warning).toBeNull()

    // Switch to a quiet session → banners clear.
    rerender({ sid: 'sess-b' })
    await waitFor(() => expect(result.current.exceeded).toBeNull())

    // Switch back → the persisted state re-shows the bar.
    rerender({ sid: 'sess-a' })
    await waitFor(() => expect(result.current.exceeded).toMatchObject({ sessionId: 'sess-a' }))
  })

  it('derives a warning at or above 80% of the cap', async () => {
    vi.mocked(api.getSessionBudget).mockResolvedValue(10)
    vi.mocked(api.getSessionUsage).mockResolvedValue({ cost_usd: 8 } as any)
    const { result } = renderHook(() => useBudgetGuard('sess-a'))
    await waitFor(() => expect(result.current.warning).toMatchObject({ spentUsd: 8, budgetUsd: 10 }))
    expect(result.current.exceeded).toBeNull()
  })

  it('shows nothing under the threshold or without a cap', async () => {
    vi.mocked(api.getSessionBudget).mockResolvedValue(10)
    vi.mocked(api.getSessionUsage).mockResolvedValue({ cost_usd: 1 } as any)
    const { result } = renderHook(() => useBudgetGuard('sess-a'))
    await waitFor(() => expect(result.current.warning).toBeNull())
    expect(result.current.exceeded).toBeNull()

    vi.mocked(api.getSessionBudget).mockResolvedValue(null)
    vi.mocked(api.getSessionUsage).mockResolvedValue({ cost_usd: 500 } as any)
    const { result: noCap } = renderHook(() => useBudgetGuard('sess-a'))
    await waitFor(() => expect(noCap.current.warning).toBeNull())
    expect(noCap.current.exceeded).toBeNull()
  })

  it('clears everything for a null session', async () => {
    const { result } = renderHook(() => useBudgetGuard(null))
    expect(api.getSessionBudget).not.toHaveBeenCalled()
    expect(result.current.warning).toBeNull()
    expect(result.current.exceeded).toBeNull()
  })

  // A-12 — the event filter used to close over currentSessionId inside the
  // subscribe effect, so every switch re-registered both listeners through
  // the ASYNC listen()/unlisten round-trip. In that window the old
  // registration was still live while its handler still compared against
  // the PREVIOUS session id — a late event for the old session passed the
  // stale filter and painted its banner onto the new session. The re-derive
  // reads stay PENDING here so the persisted re-read cannot mask the stale
  // write: the banner state is decided by the event path alone.
  it('drops the old session\u2019s late event inside the switch window (A-12 race)', async () => {
    vi.mocked(api.getSessionBudget).mockImplementation(() => new Promise(() => {}))
    vi.mocked(api.getSessionUsage).mockImplementation(() => new Promise(() => {}))
    const { result, rerender } = renderHook(({ sid }) => useBudgetGuard(sid), {
      initialProps: { sid: 'sess-old' as string | null },
    })
    await waitFor(() => expect(captured[EVENT_NAMES.BUDGET_EXCEEDED]?.length).toBeGreaterThan(0))

    // Switch to a quiet session; the old registration's unlisten is still
    // in flight when the backend emits the OLD session's exceeded event.
    rerender({ sid: 'sess-new' })
    act(() => { flush(EVENT_NAMES.BUDGET_EXCEEDED, { sessionId: 'sess-old', spentUsd: 12, budgetUsd: 10 }) })

    // The stale event must not paint the new session's banner...
    expect(result.current.exceeded).toBeNull()
    expect(result.current.warning).toBeNull()

    // ...and the session now on screen still receives its own events.
    act(() => { flush(EVENT_NAMES.BUDGET_EXCEEDED, { sessionId: 'sess-new', spentUsd: 12, budgetUsd: 10 }) })
    expect(result.current.exceeded).toMatchObject({ sessionId: 'sess-new' })
  })

  // A-12 — the handler reads the session through a ref, so a switch must
  // NOT tear down and re-register the listeners at all: no async unlisten
  // window, no churn. Asserted SYNCHRONOUSLY after the switch, while the
  // old registration's async unlisten (when one exists) is still in flight.
  it('keeps a single live registration across switches (A-12 — no re-subscribe churn)', async () => {
    const { rerender } = renderHook(({ sid }) => useBudgetGuard(sid), {
      initialProps: { sid: 'sess-a' as string | null },
    })
    await waitFor(() => expect(captured[EVENT_NAMES.BUDGET_WARNING]?.length).toBe(1))
    rerender({ sid: 'sess-b' })
    expect(captured[EVENT_NAMES.BUDGET_WARNING]).toHaveLength(1)
    expect(captured[EVENT_NAMES.BUDGET_EXCEEDED]).toHaveLength(1)
    rerender({ sid: 'sess-c' })
    expect(captured[EVENT_NAMES.BUDGET_WARNING]).toHaveLength(1)
    expect(captured[EVENT_NAMES.BUDGET_EXCEEDED]).toHaveLength(1)
  })
})
