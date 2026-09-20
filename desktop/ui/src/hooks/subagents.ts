// B2 follow-up — Tasks page sub-agent panel hook.
//
// Mirrors `goalRuns.ts` / `batchRuns.ts` (same one-refresh-per-event
// pattern, same `useTauriEvent` shape) so the new SubagentPanel slots
// into `Tasks.tsx` without a new pattern. Returns an empty list when the
// user has not enabled agent teams — the panel renders its empty state
// and the hook stays quiet.

import { useCallback, useEffect, useState } from 'react'
import * as api from '@/lib/tauri-api'
import { EVENT_NAMES, type SubAgentDto } from '@/types'
import { useTauriEvent } from '@/hooks/useTauriEvent'
import { toastError } from '@/lib/errorToast'
import { useT } from '@/i18n'

export interface UseSubagents {
  agents: SubAgentDto[]
  loading: boolean
  error: string | null
  refresh: () => Promise<void>
}

export function useSubagents(): UseSubagents {
  const t = useT()
  const [agents, setAgents] = useState<SubAgentDto[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const refresh = useCallback(async () => {
    setLoading(true)
    try {
      const next = (await api.listSubagents()) ?? []
      setAgents(next)
      setError(null)
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      setError(msg)
      // Tasks page is a passive view — never block it on a sub-agent fetch
      // failure; the toast is the right channel for transient errors.
      toastError(t('subagent.refreshFailed'), e)
    } finally {
      setLoading(false)
    }
  }, [t])

  // Mount refresh
  useEffect(() => {
    void refresh()
  }, [refresh])

  // Refresh on every lifecycle transition — both `subagent:start` and
  // `subagent:stop` change the registry shape, so a single subscription
  // per event covers both additions and removals.
  useTauriEvent(EVENT_NAMES.SUBAGENT_START, () => {
    void refresh()
  })
  useTauriEvent(EVENT_NAMES.SUBAGENT_STOP, () => {
    void refresh()
  })

  return { agents, loading, error, refresh }
}