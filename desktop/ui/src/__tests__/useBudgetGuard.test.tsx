// B4 P2-8 — the budget banners survive a session switch by re-deriving
// their state from the persisted backend budget (get_session_budget +
// get_session_usage), matching the backend's own thresholds (warning at
// >= 80% of the cap, exceeded at >= cap). Events stay the live path.
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { renderHook, waitFor } from '@testing-library/react'
import { useBudgetGuard } from '@/hooks/useBudgetGuard'
import * as api from '@/lib/tauri-api'

beforeEach(() => {
  vi.clearAllMocks()
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
})
