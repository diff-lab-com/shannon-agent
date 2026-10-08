import { useEffect } from 'react'
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import ChatInput from '@/components/chat/ChatInput'
import ChatStatusBar from '@/components/chat/ChatStatusBar'
import SlashResultCard from '@/components/chat/SlashResultCard'
import QueueChips from './QueueChips'
import { useT } from '@/i18n'
import { useChat } from '@/context/ChatContext'
import { useSessions } from '@/context/SessionContext'
import { useCatalog } from '@/context/CatalogContext'
import { changeSessionWorkingDir } from '@/lib/sessionActions'
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
  const { input, setInput, handleSend, handleSteer, attachedFiles, handleAttach, handleDetachAll, executeSlash, slashResult, dismissSlashResult, editing, cancelEdit } = useComposer()
  const { isQuerying, isCancelInFlight, cancelQuery, usage } = useChat()
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
      {/* Same reading measure as the message flow (ui-redesign-2026-10
          02-chat) — the status bar rides this container too. */}
      <div className="max-w-reading mx-auto">
        {/* Aurora signature (ui-redesign-2026-10 §1): the composer is one of
            the four surfaces allowed to carry the aurora-line — a 1px
            violet→cyan hairline on the glass top edge. Pure paint (no
            backdrop-filter), so the on-screen glass budget stays ≤ 4. */}
        <div className="glass-surface rounded-2xl aurora-line">
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
            // GB P2-10a: the interrupt-now send — bolt button / Ctrl+Enter
            // while this session streams (Enter keeps queueing).
            onSteer={handleSteer}
            onExecuteSlash={executeSlash}
            attachedFiles={attachedFiles}
            onAttach={handleAttach}
            onDetachAll={handleDetachAll}
            isQuerying={isQuerying}
            // S-3/A-18 companion: stop already in flight → disabled spinner.
            cancelInFlight={isCancelInFlight}
            onCancelQuery={cancelQuery}
            // Present only while an edit is in flight — Escape inside the
            // textarea then exits edit mode (restoring the pre-edit draft).
            onCancelEdit={editing ? cancelEdit : undefined}
            onOpenQuickFix={() => setQuickFixOpen(true)}
            onOpenEditor={() => setEditorOpen(true)}
            sessionWorkingDir={sessionWorkingDir}
            usageTick={usage}
            // R2-1: model-chip switches scope to THIS session; the chip's
            // "Set as default" stays the global write.
            sessionId={currentSessionId}
          />
        </div>
        {/* Aurora redesign 2026-10 (02-chat.html 状态条): the strip under the
            composer. Absorbs the old working-directory row (same button,
            aria label and breadcrumb) and adds the session spend / budget /
            context segments — each hides when its data source is absent
            (honesty contract, see ChatStatusBar). */}
        <ChatStatusBar
          workingDir={sessionWorkingDir}
          usage={usage}
          sessionId={currentSessionId}
          onChangeWorkingDir={() => void changeSessionWorkingDir(currentSessionId, t)}
        />
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
        className="rounded-sm hover:bg-error/10 hover:text-error shrink-0"
      >
        <span className="material-symbols-outlined icon-sm" aria-hidden="true">close</span>
      </Button>
    </div>
  )
}
