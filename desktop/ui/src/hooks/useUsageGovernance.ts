// useUsageGovernance — P2-1 usage-governance state for the sidebar % bar and
// the /usage budget card.
//
// Polls `get_usage_governance` (month-to-date spend vs the user-set monthly
// budget + 80/100% threshold state) on a slow interval and on tab
// visibility, mirroring the sidebar's inbox-stats refresh pattern. The
// backend command doubles as the threshold-alert trigger (it fires the
// once-per-month desktop notification), so polling here is what keeps the
// alerts running without any wiring into `send_message` or the executor.
//
// Failures degrade to `null` — every consumer renders nothing (or the
// no-budget fallback) rather than an error state for a telemetry nicety.

import { useCallback, useEffect, useState } from 'react'
import * as api from '@/lib/tauri-api'
import type { UsageGovernance } from '@/types'

export const USAGE_GOVERNANCE_POLL_MS = 60_000

export interface UsageGovernanceState {
  /** `null` while loading, on failure, or before the first snapshot. */
  governance: UsageGovernance | null
  refresh: () => void
}

export function useUsageGovernance(
  intervalMs: number = USAGE_GOVERNANCE_POLL_MS,
): UsageGovernanceState {
  const [governance, setGovernance] = useState<UsageGovernance | null>(null)
  const [tick, setTick] = useState(0)
  const refresh = useCallback(() => setTick(t => t + 1), [])

  useEffect(() => {
    let cancelled = false
    const load = () => {
      api.getUsageGovernance()
        .then(g => { if (!cancelled) setGovernance(g) })
        .catch(() => { if (!cancelled) setGovernance(null) })
    }
    load()
    const timer = window.setInterval(load, intervalMs)
    return () => {
      cancelled = true
      window.clearInterval(timer)
    }
  }, [intervalMs, tick])

  // Refresh when the window becomes visible again (the interval alone would
  // show stale spend after a long backgrounded stretch).
  useEffect(() => {
    if (typeof document === 'undefined') return
    const onVisibility = () => { if (!document.hidden) refresh() }
    document.addEventListener('visibilitychange', onVisibility)
    return () => { document.removeEventListener('visibilitychange', onVisibility) }
  }, [refresh])

  return { governance, refresh }
}
