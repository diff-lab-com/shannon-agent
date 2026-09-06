/**
 * P1-5 C-2 — per-project workspace layout load/save with restore semantics.
 *
 * `resolveLayout` implements the frozen rules: no saved layout → default
 * focus preset; version mismatch or structurally invalid layout → default
 * focus preset. Persistence failures are non-fatal (the workspace still
 * works, it just won't remember). Edits are ignored until the first load
 * settles so a slow `workspace_get_layout` can never be overwritten by a
 * stale default.
 */
import { useCallback, useEffect, useState } from 'react'
import * as api from '@/lib/tauri-api'
import { presetLayout, resolveLayout, type WorkspaceLayout } from './layout'

export function useWorkspaceLayout(projectKey: string | null): {
  layout: WorkspaceLayout
  /** False until the first load attempt settled (edits are deferred). */
  loaded: boolean
  update: (next: WorkspaceLayout) => void
} {
  const [layout, setLayout] = useState<WorkspaceLayout>(() => presetLayout('focus'))
  const [loaded, setLoaded] = useState(false)

  useEffect(() => {
    let cancelled = false
    if (!projectKey) {
      setLayout(presetLayout('focus'))
      setLoaded(true)
      return
    }
    api.workspaceGetLayout(projectKey)
      .then(saved => {
        if (cancelled) return
        setLayout(resolveLayout(saved))
        setLoaded(true)
      })
      .catch(() => {
        // Backend unreachable (e.g. demo mode gap): default layout, no retry.
        if (!cancelled) setLoaded(true)
      })
    return () => { cancelled = true }
  }, [projectKey])

  const update = useCallback((next: WorkspaceLayout) => {
    if (!loaded) return
    setLayout(next)
    if (projectKey) {
      void api.workspaceSetLayout(projectKey, next).catch(() => { /* non-fatal */ })
    }
  }, [loaded, projectKey])

  return { layout, loaded, update }
}
