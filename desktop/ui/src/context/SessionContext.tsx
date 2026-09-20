// SessionContext — session list + active session slice of the former
// AppContext. Provided by AppProvider, which owns the state and actions; this
// file only declares the slice type, the context, and the useSessions hook.

import { createContext, useContext } from 'react'
import type { GoalRunDto, SessionActivity, SessionInfo, SubAgentLive } from '@/types'

export interface SessionContextValue {
  sessions: SessionInfo[]
  /** P0 sidebar telemetry: live per-session activity (running / elapsed /
   *  active tool), derived from the query:* event stream and reconciled
   *  with `SessionInfo.running` on each refresh. */
  sessionActivity: Record<string, SessionActivity>
  /** P2-⑥: goal runs keyed by the session they own — drives the sidebar's
   *  goal badge + iteration progress. */
  goalRunsBySession: Record<string, GoalRunDto>
  /** B2: live state of the currently running sub-agent (subagent:start /
   *  subagent:stop bridge); null when no spawn is executing. Lives in the
   *  low-frequency session slice so SubagentBlock can subscribe without
   *  re-rendering on every streamed token. */
  subagentLive: SubAgentLive | null
  currentSessionId: string | null
  /** P1-1: session pinned to this window via `/?windowSession=<id>`; null in the main window. In-memory only. */
  windowSessionId: string | null
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
