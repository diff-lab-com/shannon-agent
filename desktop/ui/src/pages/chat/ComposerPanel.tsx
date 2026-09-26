import { useEffect } from 'react'
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import ChatInput from '@/components/chat/ChatInput'
import SlashResultCard from '@/components/chat/SlashResultCard'
import QueueChips from './QueueChips'
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
  const { input, setInput, handleSend, attachedFiles, handleAttach, handleDetachAll, executeSlash, slashResult, dismissSlashResult, editing, cancelEdit } = useComposer()
  const { isQuerying, cancelQuery, usage } = useChat()
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

  // Composer is a normal flex child pinned to the panel bottom (shrink-0) —
  // never an absolutely-positioned overlay. The old `absolute bottom-*`
  // placement escaped the viewport whenever the positioning context scrolled,
  // leaving the composer unreachable (audit P0: composer always visible).
  return (
    <div className="shrink-0 w-full px-lg md:px-xl pb-md pt-xs">
      <div className="max-w-4xl mx-auto">
        <div className="glass-surface rounded-2xl">
          {slashResult && <SlashResultCard result={slashResult} onDismiss={dismissSlashResult} />}
          {/* B1 §4-8: while editing, the banner identifies the target message
              and offers the escape hatch (restores the pre-edit draft). */}
          {editing && <EditBanner timestamp={editing.timestamp} onCancel={cancelEdit} />}
          {/* B1 §4-9: prompts queued while this session streams. */}
          <QueueChips />
          <ChatInput
            value={input}
            onChange={setInput}
            onSend={handleSend}
            onExecuteSlash={executeSlash}
            attachedFiles={attachedFiles}
            onAttach={handleAttach}
            onDetachAll={handleDetachAll}
            isQuerying={isQuerying}
            onCancelQuery={cancelQuery}
            // Present only while an edit is in flight — Escape inside the
            // textarea then exits edit mode (restoring the pre-edit draft).
            onCancelEdit={editing ? cancelEdit : undefined}
            onOpenQuickFix={() => setQuickFixOpen(true)}
            onOpenEditor={() => setEditorOpen(true)}
            sessionWorkingDir={sessionWorkingDir}
            usageTick={usage}
          />
        </div>
        {/* Single-child row — plain start alignment (the old justify-between
            implied a second trailing slot that no longer exists). */}
        <div className="mt-xs flex items-center gap-md px-sm text-label-sm text-on-surface-variant">
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

/** B1 §4-8: dismissible banner naming the message under edit (chat.edit.banner). */
function EditBanner({ timestamp, onCancel }: { timestamp: number; onCancel: () => void }) {
  const intl = useIntl()
  const t = useT()
  const time = new Date(timestamp).toLocaleTimeString(intl.locale, { hour: '2-digit', minute: '2-digit' })
  return (
    <div
      role="status"
      data-testid="edit-banner"
      className="flex items-center gap-xs px-md py-xs bg-secondary-container/40 border-b border-outline-variant/20 rounded-t-2xl text-on-surface"
    >
      <span className="material-symbols-outlined icon-sm text-secondary shrink-0" aria-hidden="true">edit</span>
      <span className="font-label-sm truncate flex-1">{t('chat.edit.banner', { time })}</span>
      <Button
        variant="ghost"
        size="icon-xs"
        onClick={onCancel}
        aria-label={t('chat.edit.cancel.aria')}
        title={t('chat.edit.cancel.aria')}
        className="rounded hover:bg-error/10 hover:text-error shrink-0"
      >
        <span className="material-symbols-outlined icon-sm" aria-hidden="true">close</span>
      </Button>
    </div>
  )
}
