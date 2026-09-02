import { useEffect } from 'react'
import { Button } from '@/components/ui/button'
import ChatInput from '@/components/chat/ChatInput'
import { useT } from '@/i18n'
import { useChat } from '@/context/ChatContext'
import { useSessions } from '@/context/SessionContext'
import { useCatalog } from '@/context/CatalogContext'
import { changeSessionWorkingDir } from '@/lib/sessionActions'
import { formatDirBreadcrumb } from './utils'
import { useComposer } from './ComposerContext'

interface ComposerPanelProps {
  setQuickFixOpen: (open: boolean) => void
  setEditorOpen: (open: boolean) => void
}

// U2: the composer footer keeps the working-directory picker (the app's only
// WD entry point) but no longer mirrors provider/model — the global Header
// is the single model surface. Composer state and the working directory both
// resolve here via contexts instead of being drilled from the page (the
// Cmd/Ctrl+D WD-picker shortcut is handled by this panel too — it owns the
// picker button).
export default function ComposerPanel({ setQuickFixOpen, setEditorOpen }: ComposerPanelProps) {
  const { input, setInput, handleSend, attachedFiles, handleAttach, handleDetachAll } = useComposer()
  const { isQuerying, cancelQuery } = useChat()
  const { sessions, currentSessionId } = useSessions()
  const { config } = useCatalog()
  const t = useT()

  const currentSession = sessions.find(s => s.id === currentSessionId)
  const sessionWorkingDir = currentSession?.working_dir ?? config?.working_dir ?? ''

  useEffect(() => {
    const handler = () => void changeSessionWorkingDir(currentSessionId, t)
    window.addEventListener('shannon:change-wd', handler)
    return () => window.removeEventListener('shannon:change-wd', handler)
  }, [currentSessionId, t])

  // U9: offset derives from the footer height token (32px) — mobile sits
  // 8px above it, ≥md 16px — instead of bare bottom-6/12 magic numbers.
  return (
    <div className="absolute bottom-[calc(var(--spacing-footer)-8px)] md:bottom-[calc(var(--spacing-footer)+16px)] w-full px-lg md:px-xl py-lg transition-colors">
      <div className="max-w-4xl mx-auto">
        <div className="bg-surface-container-lowest border border-outline-variant/30 rounded-2xl shadow-sm">
          <ChatInput
            value={input}
            onChange={setInput}
            onSend={handleSend}
            attachedFiles={attachedFiles}
            onAttach={handleAttach}
            onDetachAll={handleDetachAll}
            disabled={isQuerying}
            isQuerying={isQuerying}
            onCancelQuery={cancelQuery}
            onOpenQuickFix={() => setQuickFixOpen(true)}
            onOpenEditor={() => setEditorOpen(true)}
          />
        </div>
        <div className="mt-xs flex items-center justify-between gap-md px-sm text-label-sm text-on-surface-variant">
          <Button
            type="button"
            variant="ghost"
            onClick={() => void changeSessionWorkingDir(currentSessionId, t)}
            disabled={!currentSessionId}
            className="flex items-center gap-xs min-w-0 hover:text-primary transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            title={sessionWorkingDir || t('chat.input.footer.workingDir.unset')}
            aria-label={t('chat.input.footer.workingDir.aria')}
          >
            <span className="material-symbols-outlined text-[14px] shrink-0">folder</span>
            <span className="truncate font-mono">
              {sessionWorkingDir ? formatDirBreadcrumb(sessionWorkingDir) : t('chat.input.footer.workingDir.unset')}
            </span>
          </Button>
        </div>
      </div>
    </div>
  )
}
