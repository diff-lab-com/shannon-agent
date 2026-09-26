// P-U3 — shared plumbing for the project deep links (/tasks?project=…,
// /triage?project=…). Resolves the raw `project` search param into the
// trailing-slash-normalized project key (the same key the rail's project
// tree groups by, via SidebarSessions.projectKeyOf) and the display label
// for the removable chip (registry name ?? path tail — the T3 naming rule).
//
// The registry read is best-effort: engines (or test mocks) without
// list_projects just degrade the label to the path tail. The chip's ×
// strips the `project` param in place (replace navigation, so no history
// spam — the same convention as the Settings ?scope= chip).

import { useCallback, useEffect, useState } from 'react'
import { useSearchParams } from 'react-router-dom'
import { pathTail, projectKeyOf } from '@/components/SidebarSessions'
import * as api from '@/lib/tauri-api'
import type { ProjectRecord } from '@/types'

export function useProjectDeepLink() {
  const [searchParams, setSearchParams] = useSearchParams()
  const raw = searchParams.get('project')
  const projectKey = projectKeyOf({ working_dir: raw })

  // Display-name registry (name may be null — the tail is the fallback, the
  // same resolution the rail uses for group labels).
  const [registry, setRegistry] = useState<ProjectRecord[]>([])
  useEffect(() => {
    if (!projectKey) return
    let cancelled = false
    try {
      api.listProjects(true)
        .then(rows => { if (!cancelled) setRegistry(Array.isArray(rows) ? rows : []) })
        .catch(() => { if (!cancelled) setRegistry([]) })
    } catch { /* registry stays empty — label falls back to the tail */ }
    return () => { cancelled = true }
  }, [projectKey])

  const projectLabel = projectKey
    ? registry.find(p => projectKeyOf({ working_dir: p.path }) === projectKey)?.name
      ?? pathTail(projectKey)
    : null

  const clearProject = useCallback(() => {
    const next = new URLSearchParams(searchParams)
    next.delete('project')
    setSearchParams(next, { replace: true })
  }, [searchParams, setSearchParams])

  return { projectKey, projectLabel, clearProject }
}
