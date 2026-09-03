// ChatContext — high-frequency chat/streaming slice of the former AppContext.
//
// Holds the per-token streaming state (streamingText updates on every token),
// so only chat consumers re-render while a response streams — Sidebar,
// Settings, etc. no longer re-render on each token. Provided by AppProvider,
// which owns the actual state and actions; this file only declares the slice
// type, the context, the useChat hook, and the <ChatProvider>.
//
// chat.v2 decision (2026-09): production chat renders the legacy path only —
// the assistant-ui runtime mount and the dev-only /chat-v2-spike route were
// removed. The bridge library under src/lib/runtime/ stays as a dormant,
// tested asset; see the ChatProvider note below if that work resumes.

import { createContext, useContext, type ReactNode } from 'react'
import type { CheckpointInfo, CompactSessionResult, FeedbackRating } from '@/lib/tauri-api'
import type { ChatMessage, ToolCall, UsagePayload } from '@/types'

export interface ChatContextValue {
  messages: ChatMessage[]
  streamingText: string
  thinkingText: string
  isQuerying: boolean
  activeToolCalls: ToolCall[]
  usage: UsagePayload | null
  sendMessage: (message: string, filePaths?: string[]) => Promise<void>
  cancelQuery: () => Promise<void>
  /** /rewind: completed checkpoints for the current session (turn indices). */
  checkpoints: CheckpointInfo[]
  /** Rewind to before `turnIndex`: drops that turn and everything after. */
  rewindSession: (turnIndex: number) => Promise<void>
  /** /compact: summarize history; resolves with the summary + new messages. */
  compactSession: () => Promise<CompactSessionResult>
  /** PM-12: persisted message ratings for the current session. */
  feedback: Record<string, FeedbackRating>
  /** Set/clear a message's rating (null clears). Optimistic, then persisted. */
  recordFeedback: (key: string, rating: FeedbackRating | null) => Promise<void>
  /** U2: ContextPanel open state lives here so the global Header (in
   * Layout, outside the /chat route) can toggle the panel that Chat renders. */
  contextPanelOpen: boolean
  toggleContextPanel: () => void
}

export const ChatContext = createContext<ChatContextValue | null>(null)

export function useChat(): ChatContextValue {
  const ctx = useContext(ChatContext)
  if (!ctx) throw new Error('useChat must be used within AppProvider')
  return ctx
}

/**
 * Provider for the chat slice. chat.v2 decision (2026-09): production chat
 * renders the legacy path only — the assistant-ui runtime mount and the
 * dev-only /chat-v2-spike route were removed. The bridge library under
 * src/lib/runtime/ stays as a dormant, tested asset for a future upgrade;
 * re-wrap children in `ChatV2RuntimeProvider` if that work resumes.
 */
export function ChatProvider({
  value,
  children,
}: {
  value: ChatContextValue
  children: ReactNode
}) {
  return <ChatContext.Provider value={value}>{children}</ChatContext.Provider>
}
