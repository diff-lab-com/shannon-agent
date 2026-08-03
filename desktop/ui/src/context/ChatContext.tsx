// ChatContext — high-frequency chat/streaming slice of the former AppContext.
//
// Holds the per-token streaming state (streamingText updates on every token),
// so only chat consumers re-render while a response streams — Sidebar,
// Settings, etc. no longer re-render on each token. Provided by AppProvider,
// which owns the actual state and actions; this file only declares the slice
// type, the context, the useChat hook, and the <ChatProvider>.
//
// P2-5a: <ChatProvider> wraps its children with
// `<ChatV2RuntimeProvider>` so the assistant-ui runtime mounts (gated on the
// `chat.v2` feature flag) without disturbing the slice contract that 19
// existing consumers rely on. When the flag is OFF, ChatV2RuntimeProvider
// is a passthrough — the runtime never mounts and `<ChatContext.Provider>`
// behaves exactly as it did before P2-5a. See `docs/plans/chat-upgrade.md`
// §3.1 acceptance criterion #3.

import { createContext, useContext, type ReactNode } from 'react'
import { ChatV2RuntimeProvider } from '@/lib/runtime/ChatV2RuntimeProvider'
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

/**
 * Provider for the chat slice. P2-5a: when chat.v2 is on, children also sit
 * inside the assistant-ui `<AssistantRuntimeProvider>`. The slice contract
 * (context value shape, `useChat()` return) is unchanged.
 */
export function ChatProvider({
  value,
  children,
}: {
  value: ChatContextValue
  children: ReactNode
}) {
  return (
    <ChatV2RuntimeProvider>
      <ChatContext.Provider value={value}>{children}</ChatContext.Provider>
    </ChatV2RuntimeProvider>
  )
}
