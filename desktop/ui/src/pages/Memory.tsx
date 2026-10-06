// Memory page — thin wrapper around MemoryPanel.
//
// MemoryPanel is the real surface; this wrapper exists so react-router can
// lazy-load it as a page-level route (`/memory`).
//
// P2-4: this page owns the provenance jump (Memory entry → source chat
// session): the frozen `get_memory_source` command resolves the stored source
// session, then the existing `switchSession` navigates to /chat. The page
// sits inside Router + AppProvider, which MemoryPanel (also rendered bare in
// unit tests) does not require.

import { useCallback, useContext, useEffect, useState } from 'react'
import { useIntl } from 'react-intl'
import { useNavigate, useSearchParams } from 'react-router-dom'
import { toast } from 'sonner'
import MemoryPanel from '@/components/memory/MemoryPanel'
import { getMemorySource, listMemoryProjects } from '@/lib/tauri-api'
import { SessionContext } from '@/context/SessionContext'

export default function Memory() {
  const intl = useIntl()
  const navigate = useNavigate()
  const sessionCtx = useContext(SessionContext)

  // P-U3: /memory?project=<path> presets the page's EXISTING project filter
  // (no new filter UI). Memory projects are labels, not paths, so the param
  // resolves against listMemoryProjects: exact match first, then a unique
  // path-tail match; anything else leaves the filter at 全部. Resolution is
  // async, so the panel consumes it as a prop and applies it when it lands.
  const [searchParams] = useSearchParams()
  const projectParam = searchParams.get('project')
  const [projectPreset, setProjectPreset] = useState<string | null>(null)
  useEffect(() => {
    if (!projectParam) return
    let cancelled = false
    try {
      listMemoryProjects()
        .then(projects => {
          if (cancelled) return
          const tail = projectParam.split(/[\\/]/).filter(Boolean).pop() ?? projectParam
          const matched = (projects ?? []).includes(projectParam)
            ? projectParam
            : (projects ?? []).filter(p => p === tail).length === 1
              ? tail
              : null
          setProjectPreset(matched)
        })
        .catch(() => { /* filter stays at 全部 */ })
    } catch { /* filter stays at 全部 */ }
    return () => { cancelled = true }
  }, [projectParam])

  const handleOpenMemorySource = useCallback(
    async (memoryId: string) => {
      try {
        const source = await getMemorySource(sessionCtx?.currentSessionId ?? null, memoryId)
        if (!source?.sessionId) {
          toast.error(intl.formatMessage({ id: 'memory.source.unavailable' }))
          return
        }
        await sessionCtx?.switchSession(source.sessionId)
        navigate('/chat')
      } catch (e) {
        toast.error(
          e instanceof Error
            ? e.message
            : intl.formatMessage({ id: 'memory.source.unavailable' }),
        )
      }
    },
    [intl, navigate, sessionCtx],
  )

  return <MemoryPanel onOpenMemorySource={handleOpenMemorySource} projectPreset={projectPreset} />
}
