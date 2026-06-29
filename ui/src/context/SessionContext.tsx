// SessionContext — session list + active session slice of the former
// AppContext. Provided by AppProvider, which owns the state and actions; this
// file only declares the slice type, the context, and the useSessions hook.

import { createContext, useContext } from 'react'
import type { SessionInfo } from '@/types'

export interface SessionContextValue {
  sessions: SessionInfo[]
  currentSessionId: string | null
  createSession: () => Promise<void>
  createSessionInWorktree: () => Promise<void>
  switchSession: (id: string) => Promise<void>
  deleteSession: (id: string) => Promise<void>
  renameSession: (id: string, title: string) => Promise<void>
  refreshSessions: () => Promise<void>
}

export const SessionContext = createContext<SessionContextValue | null>(null)

export function useSessions(): SessionContextValue {
  const ctx = useContext(SessionContext)
  if (!ctx) throw new Error('useSessions must be used within AppProvider')
  return ctx
}
