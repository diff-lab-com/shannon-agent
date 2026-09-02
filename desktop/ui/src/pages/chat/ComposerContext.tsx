// Composer state (draft text + attachments) lives here instead of being
// drilled Chat → MessageArea / ComposerPanel → ChatInput. Input updates are
// high-frequency, so this is deliberately a page-local context and NOT part
// of the chat slice — useChat() consumers (the message list) must not
// re-render on every keystroke.
import { createContext, useContext } from 'react'

export interface ComposerContextValue {
  input: string
  setInput: (s: string) => void
  handleSend: () => void
  attachedFiles: string[]
  handleAttach: (files: string[]) => void
  handleDetachAll: () => void
}

export const ComposerContext = createContext<ComposerContextValue | null>(null)

export function useComposer(): ComposerContextValue {
  const ctx = useContext(ComposerContext)
  if (!ctx) throw new Error('useComposer must be used within ComposerContext.Provider')
  return ctx
}
