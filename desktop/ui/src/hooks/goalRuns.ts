// React hooks wrapping the P0-2 desktop goal runner commands.
//
// Mirrors the shape of the inbox hooks in `hooks/inbox.ts`: the hook owns
// loading/error state, exposes action functions that toast on failure, and
// re-fetches when the backend emits `goal:updated` (run started, a turn
// finished, paused/resumed/stopped, restart reconciliation) so the
// Tasks-page run cards stay live without polling.

import { useCallback, useEffect, useState } from 'react'
import { useT } from '@/i18n'
import { toast } from 'sonner'
import { toastError } from '@/lib/errorToast'
import * as api from '@/lib/tauri-api'
import type { GoalRunDto, GoalRunStatus } from '@/types'
import { EVENT_NAMES } from '@/types'
import { useTauriEvent } from '@/hooks/useTauriEvent'

const BLOCKING_STATUSES: GoalRunStatus[] = ['running', 'paused']

export function isGoalRunBlocking(run: Pick<GoalRunDto, 'status'>): boolean {
  return BLOCKING_STATUSES.includes(run.status)
}

export function useGoalRuns() {
  const t = useT()
  const [runs, setRuns] = useState<GoalRunDto[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const refresh = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      setRuns((await api.listGoalRuns()) ?? [])
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      setError(msg)
      console.warn('useGoalRuns.refresh failed:', e)
    } finally {
      setLoading(false)
    }
  }, [])

  const act = useCallback(
    async (
      id: string,
      action: (sessionId: string) => Promise<void>,
      successKey: string,
      failureKey: string,
    ): Promise<boolean> => {
      try {
        await action(id)
        toast.success(t(successKey))
        await refresh()
        return true
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e)
        setError(msg)
        toastError(t(failureKey), e)
        return false
      }
    },
    [refresh, t],
  )

  const pause = useCallback(
    (id: string) => act(id, api.pauseGoalRun, 'goal.toast.paused', 'goal.toast.failed.pause'),
    [act],
  )
  const resume = useCallback(
    (id: string) => act(id, api.resumeGoalRun, 'goal.toast.resumed', 'goal.toast.failed.resume'),
    [act],
  )
  const stop = useCallback(
    (id: string) => act(id, api.stopGoalRun, 'goal.toast.stopped', 'goal.toast.failed.stop'),
    [act],
  )

  const start = useCallback(
    async (input: api.GoalRunStartInput & { sessionId?: string | null }) => {
      try {
        const { sessionId } = await api.startGoalRun(input)
        toast.success(t('goal.toast.started'))
        await refresh()
        return sessionId
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e)
        setError(msg)
        toastError(t('goal.toast.failed.start'), e)
        return null
      }
    },
    [refresh, t],
  )

  const updateObjective = useCallback(
    async (id: string, objective: string): Promise<boolean> => {
      try {
        await api.updateGoalObjective(id, objective)
        toast.success(t('goal.toast.objectiveUpdated'))
        await refresh()
        return true
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e)
        setError(msg)
        toastError(t('goal.toast.failed.objective'), e)
        return false
      }
    },
    [refresh, t],
  )

  useEffect(() => { refresh() }, [refresh])

  // Backend pushes `goal:updated` on every run-state change — refresh so
  // mounted run cards never go stale.
  useTauriEvent(EVENT_NAMES.GOAL_UPDATED, () => { void refresh() })

  return { runs, loading, error, refresh, start, pause, resume, stop, updateObjective }
}
