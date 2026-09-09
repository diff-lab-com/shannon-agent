// useBudgetGuard — P0-4 listener for the session budget events.
//
// Subscribes to `budget:warning` (>= 80% of the cap, yellow advisory bar)
// and `budget:exceeded` (cap hit — red choice bar) and keeps the most
// recent payload *for the current session* so unrelated sessions' events
// never surface here. The banner is the consumer; `dismiss` clears one or
// both banners.

import { useCallback, useEffect, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import { EVENT_NAMES, type BudgetStatusPayload } from '@/types'

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

  // A session switch invalidates both banners.
  useEffect(() => {
    setWarning(null)
    setExceeded(null)
  }, [currentSessionId])

  const clearWarning = useCallback(() => setWarning(null), [])
  const clearExceeded = useCallback(() => setExceeded(null), [])

  return { warning, exceeded, clearWarning, clearExceeded }
}
