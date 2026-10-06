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
  /** B1 P2-3: true while a session-switch IPC is in flight (drives the
   *  message-area skeleton). Never set for same-session remounts. */
  switchingSession: boolean
  createSession: () => Promise<void>
  createSessionInWorktree: () => Promise<void>
  switchSession: (id: string) => Promise<void>
  deleteSession: (id: string) => Promise<void>
  renameSession: (id: string, title: string) => Promise<void>
  refreshSessions: () => Promise<void>
  /**
   * office Wave 3 C4: per-session scratchpad of source references (file
   * paths / URLs) kept by the RightDock Context tab's "Session sources"
   * block. Draft-board semantics on purpose — in-memory only (a refresh
   * clears it), it never touches the send_message pipeline; the real
   * always-injected source set is a later wave. Keyed by session id.
   */
  sessionSources: Record<string, string[]>
  /** Append one source (trimmed, deduped) to a session's list. */
  addSessionSource: (sessionId: string, item: string) => void
  /** Remove one source (by value) from a session's list. */
  removeSessionSource: (sessionId: string, item: string) => void
  /** B3-1 (P1-2, R9-②): queued-prompt count per session — the sidebar's
   *  「队列 N」badge. Deliberately depth-only: the full FIFO stays the
   *  visible session's slice (ChatContext.promptQueue) and the drain keeps
   *  its Chat-page semantics; this just makes a background session's parked
   *  backlog visible on the rail. Sessions without a queue are absent. */
  queueDepthsBySession: Record<string, number>
}

export const SessionContext = createContext<SessionContextValue | null>(null)

export function useSessions(): SessionContextValue {
  const ctx = useContext(SessionContext)
  if (!ctx) throw new Error('useSessions must be used within AppProvider')
  return ctx
}
