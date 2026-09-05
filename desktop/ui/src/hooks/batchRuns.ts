// React hooks wrapping the P1-2 desktop best-of-N batch commands.
//
// Mirrors `hooks/goalRuns.ts`: the hook owns loading/error state, exposes
// start/adopt/discard actions that toast on failure, and re-fetches when the
// backend emits `batch:updated` (batch started, a branch finished, live
// spend, adopted/discarded) so the Tasks-page batch cards stay live without
// polling.

import { useCallback, useEffect, useState } from 'react'
import { useT } from '@/i18n'
import { toast } from 'sonner'
import { toastError } from '@/lib/errorToast'
import * as api from '@/lib/tauri-api'
import type { BatchRunDto } from '@/types'
import { EVENT_NAMES } from '@/types'
import { useTauriEvent } from '@/hooks/useTauriEvent'

/** A batch with all branches terminal — compare/adopt/discard unlock. */
export function isBatchTerminal(run: Pick<BatchRunDto, 'status'>): boolean {
  return run.status !== 'running'
}

/** Branches the user can adopt (the backend enforces this too). */
export function adoptableBranches(run: BatchRunDto) {
  return run.branches.filter(b => b.status === 'completed')
}

export function useBatchRuns() {
  const t = useT()
  const [runs, setRuns] = useState<BatchRunDto[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const refresh = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      setRuns((await api.listBatchRuns()) ?? [])
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      setError(msg)
      console.warn('useBatchRuns.refresh failed:', e)
    } finally {
      setLoading(false)
    }
  }, [])

  const start = useCallback(
    async (input: api.BatchRunStartInput): Promise<string | null> => {
      try {
        const { batchId } = await api.startBatchRun(input)
        toast.success(t('batch.toast.started'))
        await refresh()
        return batchId
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e)
        setError(msg)
        toastError(t('batch.toast.failed.start'), e)
        return null
      }
    },
    [refresh, t],
  )

  const adopt = useCallback(
    async (
      batchId: string,
      index: number,
    ): Promise<{ merged: boolean; conflicts: string[] | null } | null> => {
      try {
        const result = await api.adoptBatchBranch(batchId, index)
        if (result.merged) {
          toast.success(t('batch.toast.adopted'))
        } else {
          // Conflicts: the backend kept every worktree; the caller surfaces
          // the file list (we toast a short hint too).
          toast.warning(t('batch.toast.conflicts'))
        }
        await refresh()
        return result
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e)
        setError(msg)
        toastError(t('batch.toast.failed.adopt'), e)
        return null
      }
    },
    [refresh, t],
  )

  const discard = useCallback(
    async (batchId: string): Promise<boolean> => {
      try {
        const result = await api.discardBatchRun(batchId)
        toast.success(t('batch.toast.discarded', { count: result.removed }))
        await refresh()
        return true
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e)
        setError(msg)
        toastError(t('batch.toast.failed.discard'), e)
        return false
      }
    },
    [refresh, t],
  )

  useEffect(() => {
    void refresh()
  }, [refresh])

  // Backend pushes `batch:updated` on every batch change (start, per-branch
  // spend + terminal, adopt, discard) — refresh so mounted cards never stall.
  useTauriEvent(EVENT_NAMES.BATCH_UPDATED, () => {
    void refresh()
  })

  return { runs, loading, error, refresh, start, adopt, discard }
}
