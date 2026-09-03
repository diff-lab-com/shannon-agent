// Composer state (draft text + attachments + slash-command surface) lives
// here instead of being drilled Chat → MessageArea / ComposerPanel →
// ChatInput. Input updates are high-frequency, so this is deliberately a
// page-local context and NOT part of the chat slice — useChat() consumers
// (the message list) must not re-render on every keystroke.
import { createContext, useContext } from 'react'
import type { SlashCommand, SlashResult } from '@/lib/slash/commands'

export interface ComposerContextValue {
  input: string
  setInput: (s: string) => void
  handleSend: () => void
  attachedFiles: string[]
  handleAttach: (files: string[]) => void
  handleDetachAll: () => void
  /** Runs a slash command resolved from the composer. */
  executeSlash: (cmd: SlashCommand) => void
  /** Output of the last slash command; rendered by ComposerPanel. */
  slashResult: SlashResult | null
  dismissSlashResult: () => void
}

export const ComposerContext = createContext<ComposerContextValue | null>(null)

export function useComposer(): ComposerContextValue {
  const ctx = useContext(ComposerContext)
  if (!ctx) throw new Error('useComposer must be used within ComposerContext.Provider')
  return ctx
}
