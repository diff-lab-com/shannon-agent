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

/** B1 §4-9: a prompt held back while its session was still streaming. */
export interface PromptQueueItem {
  id: number
  text: string
  attachments: string[]
}

export interface ChatContextValue {
  messages: ChatMessage[]
  streamingText: string
  thinkingText: string
  /**
   * B1 P1-5: query state of the VISIBLE session only (windowSessionId ??
   * currentSessionId). A background session's run no longer disables this
   * session's composer — the full per-session map stays internal.
   */
  isQuerying: boolean
  activeToolCalls: ToolCall[]
  usage: UsagePayload | null
  /**
   * `options.budgetBypass` is the "continue (ignore once)" choice from the
   * budget-exceeded banner — it exempts exactly that send's pre-turn
   * budget check (the mid-turn cap stays enforced backend-side).
   */
  sendMessage: (
    message: string,
    filePaths?: string[],
    options?: { budgetBypass?: boolean },
  ) => Promise<void>
  cancelQuery: () => Promise<void>
  /** B1 §4-9: this session's FIFO of prompts queued while streaming. */
  promptQueue: PromptQueueItem[]
  /** Append to the visible session's queue. False when the queue is full
   *  (an overflow toast is raised here; the caller keeps the draft). */
  enqueuePrompt: (text: string, attachments: string[]) => boolean
  /** Take the head of the visible session's queue (drain step). */
  dequeuePrompt: () => PromptQueueItem | null
  /** Remove one queued item by id (queue chip dismiss). */
  removeQueuedPrompt: (id: number) => void
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
  /** U2: dock open state lives here so the global Header (in Layout,
   * outside the /chat route) can toggle the dock that Chat renders.
   * B1 P1-13: persisted to `shannon.dock.open` — every path below funnels
   * through the same persisted setter.
   * P1-⑦: Chat also sets it directly — RightDock auto-docks itself on
   * plan-mode entry / artifact detection / a "Diff" click. */
  contextPanelOpen: boolean
  toggleContextPanel: () => void
  setContextPanelOpen: (open: boolean) => void
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
