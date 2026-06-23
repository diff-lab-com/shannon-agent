// Live task step streaming hook (P1.2).
//
// Listens to `task-step` and `task-retry` Tauri events and maintains a
// per-task list of step events. UI surfaces can read the latest state
// for a given task id without subscribing to the raw event stream.
//
// The hook is intentionally pure-UI state: it does not re-fetch the
// task list, just overlays step events on top of whatever the caller
// already has. When the user navigates away from a task, the caller
// should drop the entry from the returned map (or just stop reading).

import { useEffect, useState, useCallback } from 'react'
import { listen } from '@tauri-apps/api/event'

export interface TaskStep {
  stepIndex: number
  stepTotal: number
  stepLabel: string
  status: 'started' | 'completed' | 'failed'
  error: string | null
  timestampMs: number
}

export interface TaskRetry {
  attempt: number
  maxAttempts: number
  delayMs: number
  lastError: string
  timestampMs: number
}

export interface TaskStreamState {
  steps: TaskStep[]
  latestStep: TaskStep | null
  retries: TaskRetry[]
  latestRetry: TaskRetry | null
}

type StreamMap = Map<string, TaskStreamState>

const EMPTY_STATE: TaskStreamState = {
  steps: [],
  latestStep: null,
  retries: [],
  latestRetry: null,
}

interface RawTaskStepEvent {
  task_id: string
  run_id: string
  step_index: number
  step_total: number
  step_label: string
  status: string
  error: string | null
  timestamp_ms: number
}

interface RawTaskRetryEvent {
  task_id: string
  run_id: string
  attempt: number
  max_attempts: number
  delay_ms: number
  last_error: string
  timestamp_ms: number
}

export function useTaskStreaming(taskIds: string[]) {
  const [streams, setStreams] = useState<StreamMap>(new Map())

  const upsertStep = useCallback((taskId: string, step: TaskStep) => {
    setStreams(prev => {
      const next = new Map(prev)
      const current = next.get(taskId) ?? EMPTY_STATE
      next.set(taskId, {
        ...current,
        steps: [...current.steps, step],
        latestStep: step,
      })
      return next
    })
  }, [])

  const upsertRetry = useCallback((taskId: string, retry: TaskRetry) => {
    setStreams(prev => {
      const next = new Map(prev)
      const current = next.get(taskId) ?? EMPTY_STATE
      next.set(taskId, {
        ...current,
        retries: [...current.retries, retry],
        latestRetry: retry,
      })
      return next
    })
  }, [])

  useEffect(() => {
    let unlistenStep: (() => void) | undefined
    let unlistenRetry: (() => void) | undefined
    let cancelled = false
    ;(async () => {
      unlistenStep = await listen<RawTaskStepEvent>('task-step', e => {
        const p = e.payload
        const status = p.status === 'completed' || p.status === 'failed' || p.status === 'started'
          ? (p.status as TaskStep['status'])
          : 'started'
        upsertStep(p.task_id, {
          stepIndex: p.step_index,
          stepTotal: p.step_total,
          stepLabel: p.step_label,
          status,
          error: p.error,
          timestampMs: p.timestamp_ms,
        })
      })
      if (cancelled) {
        unlistenStep()
        return
      }
      unlistenRetry = await listen<RawTaskRetryEvent>('task-retry', e => {
        const p = e.payload
        upsertRetry(p.task_id, {
          attempt: p.attempt,
          maxAttempts: p.max_attempts,
          delayMs: p.delay_ms,
          lastError: p.last_error,
          timestampMs: p.timestamp_ms,
        })
      })
    })()
    return () => {
      cancelled = true
      unlistenStep?.()
      unlistenRetry?.()
    }
  }, [upsertStep, upsertRetry])

  // Drop streams the caller no longer cares about. Keeps memory bounded
  // when the user closes task panels.
  useEffect(() => {
    const keep = new Set(taskIds)
    setStreams(prev => {
      let changed = false
      const next = new Map<string, TaskStreamState>()
      for (const [id, state] of prev) {
        if (keep.has(id)) {
          next.set(id, state)
        } else {
          changed = true
        }
      }
      return changed ? next : prev
    })
  }, [taskIds])

  const clear = useCallback((taskId: string) => {
    setStreams(prev => {
      if (!prev.has(taskId)) return prev
      const next = new Map(prev)
      next.delete(taskId)
      return next
    })
  }, [])

  return { streams, clear }
}
