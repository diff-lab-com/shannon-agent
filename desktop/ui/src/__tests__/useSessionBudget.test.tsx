// P2-7 — useSessionBudget's event listeners follow the useBudgetGuard ref
// pattern: one registration for the hook's lifetime, ownership checked
// against a latest-ref at delivery time. Before this, every session switch
// tore down and re-registered both listeners through the async
// listen()/unlisten round-trip, and an event landing in that gap was
// silently dropped — the usage badge then went stale until the next event.
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { renderHook, waitFor, act } from '@testing-library/react'
import { listen } from '@tauri-apps/api/event'
import { useSessionBudget } from '@/hooks/useSessionBudget'
import { EVENT_NAMES } from '@/types'
import * as api from '@/lib/tauri-api'

// Capturing listen() fake with an ASYNC unlisten, mirroring the real bridge
// (same harness as useBudgetGuard.test): after a switch the OLD registration
// stays live until its unlisten promise resolves — exactly the dropped-event
// window this hook used to have.
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

const usage = (costUsd: number) => ({
  input_tokens: 0, output_tokens: 0, cache_creation_tokens: 0,
  cache_read_tokens: 0, cost_usd: costUsd, events: 0,
})

beforeEach(() => {
  vi.clearAllMocks()
  for (const key of Object.keys(captured)) delete captured[key]
  vi.mocked(api.getSessionBudget).mockResolvedValue(null)
  vi.mocked(api.getSessionUsage).mockResolvedValue(usage(0))
})

describe('useSessionBudget — ref-pattern listeners (P2-7)', () => {
  it('refreshes the NEW session when an event lands during a switch', async () => {
    let spent = 1
    vi.mocked(api.getSessionBudget).mockResolvedValue(10)
    vi.mocked(api.getSessionUsage).mockImplementation(async () => usage(spent))

    const { result, rerender } = renderHook(({ sid }) => useSessionBudget(sid), {
      initialProps: { sid: 'sess-a' as string | null },
    })
    await waitFor(() => expect(result.current.usage?.cost_usd).toBe(1))

    // Switch sessions — the old registrations are still live (async
    // unlisten), which is precisely when the closure filter dropped events.
    rerender({ sid: 'sess-b' })

    // A warning for the NEW session arrives mid-switch: the ref filter
    // matches, refresh re-reads and the badge shows the new spend.
    spent = 5
    await act(async () => { flush(EVENT_NAMES.BUDGET_WARNING, { sessionId: 'sess-b' }) })
    await waitFor(() => expect(result.current.usage?.cost_usd).toBe(5))

    // An event for the OTHER session never refreshes.
    spent = 9
    await act(async () => { flush(EVENT_NAMES.BUDGET_EXCEEDED, { sessionId: 'sess-a' }) })
    await new Promise(r => setTimeout(r, 0))
    expect(result.current.usage?.cost_usd).toBe(5)
  })

  it('registers each listener exactly once across session switches', async () => {
    const { rerender } = renderHook(({ sid }) => useSessionBudget(sid), {
      initialProps: { sid: 'sess-a' as string | null },
    })
    await waitFor(() => expect(listen).toHaveBeenCalledTimes(2)) // warning + exceeded
    rerender({ sid: 'sess-b' })
    rerender({ sid: 'sess-c' })
    await new Promise(r => setTimeout(r, 0))
    expect(listen).toHaveBeenCalledTimes(2)
  })
})
