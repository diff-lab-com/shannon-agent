// useSessionBudget — P0-4 session-budget state for the chat surfaces.
//
// Loads the current session's USD cap (`get_session_budget`, sidecar-backed)
// and its cumulative ledger spend + cache tokens (`get_session_usage`), and
// re-fetches on session switch, on `budget:warning` / `budget:exceeded`
// events, and whenever callers ask via `refresh` (e.g. after the budget
// dialog saves).

import { useCallback, useEffect, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import * as api from '@/lib/tauri-api'
import { EVENT_NAMES, type BudgetStatusPayload } from '@/types'
import type { SessionUsageSummary } from '@/lib/tauri-api'

export interface SessionBudgetState {
  /** Configured cap, `null` when none (or no session). */
  budget: number | null
  /** Cumulative session spend + cache tokens, `null` with no session. */
  usage: SessionUsageSummary | null
  refresh: () => void
}

export function useSessionBudget(currentSessionId: string | null): SessionBudgetState {
  const [budget, setBudget] = useState<number | null>(null)
  const [usage, setUsage] = useState<SessionUsageSummary | null>(null)
  const [tick, setTick] = useState(0)
  const refresh = useCallback(() => setTick(t => t + 1), [])

  useEffect(() => {
    let cancelled = false
    if (!currentSessionId) {
      setBudget(null)
      setUsage(null)
      return
    }
    void api.getSessionBudget(currentSessionId).then(b => { if (!cancelled) setBudget(b) }).catch(() => {})
    void api.getSessionUsage(currentSessionId).then(u => { if (!cancelled) setUsage(u) }).catch(() => {})
    return () => { cancelled = true }
  }, [currentSessionId, tick])

  // Budget events imply spend moved (or the cap changed via the banner's
  // raise action) — refresh so badges stay live without polling.
  useEffect(() => {
    if (!currentSessionId) return
    const unlisteners: Promise<() => void>[] = [
      listen<BudgetStatusPayload>(EVENT_NAMES.BUDGET_WARNING, e => {
        if (e.payload.sessionId === currentSessionId) refresh()
      }),
      listen<BudgetStatusPayload>(EVENT_NAMES.BUDGET_EXCEEDED, e => {
        if (e.payload.sessionId === currentSessionId) refresh()
      }),
    ]
    return () => { unlisteners.forEach(p => void p.then(fn => fn())) }
  }, [currentSessionId, refresh])

  return { budget, usage, refresh }
}
