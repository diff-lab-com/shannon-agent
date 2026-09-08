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

import { useCallback, useContext } from 'react'
import { useIntl } from 'react-intl'
import { useNavigate } from 'react-router-dom'
import { toast } from 'sonner'
import MemoryPanel from '@/components/memory/MemoryPanel'
import { getMemorySource } from '@/lib/tauri-api'
import { SessionContext } from '@/context/SessionContext'

export default function Memory() {
  const intl = useIntl()
  const navigate = useNavigate()
  const sessionCtx = useContext(SessionContext)

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

  return <MemoryPanel onOpenMemorySource={handleOpenMemorySource} />
}
