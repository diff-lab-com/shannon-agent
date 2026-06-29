// ChatContext — high-frequency chat/streaming slice of the former AppContext.
//
// Holds the per-token streaming state (streamingText updates on every token),
// so only chat consumers re-render while a response streams — Sidebar,
// Settings, etc. no longer re-render on each token. Provided by AppProvider,
// which owns the actual state and actions; this file only declares the slice
// type, the context, and the useChat hook.

import { createContext, useContext } from 'react'
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
}

export const ChatContext = createContext<ChatContextValue | null>(null)

export function useChat(): ChatContextValue {
  const ctx = useContext(ChatContext)
  if (!ctx) throw new Error('useChat must be used within AppProvider')
  return ctx
}
