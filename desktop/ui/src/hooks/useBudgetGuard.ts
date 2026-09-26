// useBudgetGuard — P0-4 listener for the session budget events.
//
// Subscribes to `budget:warning` (>= 80% of the cap, yellow advisory bar)
// and `budget:exceeded` (cap hit — red choice bar) and keeps the most
// recent payload *for the current session* so unrelated sessions' events
// never surface here. The banner is the consumer; `dismiss` clears one or
// both banners.
//
// B4 P2-8: the banners used to be window-local state wiped on every session
// switch, so switching back to an over-budget session stayed silent until
// its next event. The durable state lives backend-side (`get_session_budget`
// sidecar cap + `get_session_usage` cumulative ledger; the events are
// transient notifications), so on every switch we re-derive the banner state
// from that cheap read — matching the backend's own thresholds in
// `cost_commands.rs` (warning at >= 80% of the cap, exceeded at >= cap).

import { useCallback, useEffect, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import * as api from '@/lib/tauri-api'
import { EVENT_NAMES, type BudgetStatusPayload } from '@/types'

/** Mirrors `BUDGET_WARNING_FRACTION` in desktop/src/cost_commands.rs. */
const BUDGET_WARNING_FRACTION = 0.8

export interface BudgetGuardState {
  warning: BudgetStatusPayload | null
  exceeded: BudgetStatusPayload | null
  clearWarning: () => void
  clearExceeded: () => void
}

export function useBudgetGuard(currentSessionId: string | null): BudgetGuardState {
  const [warning, setWarning] = useState<BudgetStatusPayload | null>(null)
  const [exceeded, setExceeded] = useState<BudgetStatusPayload | null>(null)

  useEffect(() => {
    const unlisteners: Promise<() => void>[] = [
      listen<BudgetStatusPayload>(EVENT_NAMES.BUDGET_WARNING, e => {
        if (e.payload.sessionId === currentSessionId) setWarning(e.payload)
      }),
      listen<BudgetStatusPayload>(EVENT_NAMES.BUDGET_EXCEEDED, e => {
        if (e.payload.sessionId === currentSessionId) setExceeded(e.payload)
      }),
    ]
    return () => { unlisteners.forEach(p => void p.then(fn => fn())) }
  }, [currentSessionId])

  // On switch (and on mount), re-derive the banners from the persisted
  // budget state instead of dropping them. A session still past its cap
  // keeps its red bar when the user comes back; a fresh read of an
  // under-budget session clears whatever the previous session left behind.
  useEffect(() => {
    let cancelled = false
    if (!currentSessionId) {
      setWarning(null)
      setExceeded(null)
      return
    }
    void Promise.all([api.getSessionBudget(currentSessionId), api.getSessionUsage(currentSessionId)])
      .then(([cap, usage]) => {
        if (cancelled) return
        const spent = usage?.cost_usd ?? 0
        if (cap != null && cap > 0 && spent >= cap) {
          setWarning(null)
          setExceeded({ sessionId: currentSessionId, spentUsd: spent, budgetUsd: cap })
        } else if (cap != null && cap > 0 && spent >= cap * BUDGET_WARNING_FRACTION) {
          setExceeded(null)
          setWarning({ sessionId: currentSessionId, spentUsd: spent, budgetUsd: cap })
        } else {
          setWarning(null)
          setExceeded(null)
        }
      })
      .catch(() => { /* sidecar unreadable — banners simply stay hidden */ })
    return () => { cancelled = true }
  }, [currentSessionId])

  const clearWarning = useCallback(() => setWarning(null), [])
  const clearExceeded = useCallback(() => setExceeded(null), [])

  return { warning, exceeded, clearWarning, clearExceeded }
}
