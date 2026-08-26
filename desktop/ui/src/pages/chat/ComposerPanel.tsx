import { Button } from '@/components/ui/button'
import ChatInput from '@/components/chat/ChatInput'
import { formatDirBreadcrumb } from './utils'

interface ComposerPanelProps {
  t: (id: string) => string
  input: string
  setInput: (s: string) => void
  handleSend: () => void
  attachedFiles: string[]
  handleAttach: (files: string[]) => void
  handleDetachAll: () => void
  isQuerying: boolean
  cancelQuery: () => Promise<void>
  currentSessionId: string | null
  sessionWorkingDir: string
  handleChangeWorkingDir: () => Promise<void>
  setQuickFixOpen: (open: boolean) => void
  setEditorOpen: (open: boolean) => void
}

// U2: the composer footer keeps the working-directory picker (the app's only
// WD entry point) but no longer mirrors provider/model — the global Header
// is the single model surface.
export default function ComposerPanel({
  t,
  input,
  setInput,
  handleSend,
  attachedFiles,
  handleAttach,
  handleDetachAll,
  isQuerying,
  cancelQuery,
  currentSessionId,
  sessionWorkingDir,
  handleChangeWorkingDir,
  setQuickFixOpen,
  setEditorOpen,
}: ComposerPanelProps) {
  return (
    <div className="absolute bottom-6 md:bottom-12 w-full px-lg md:px-xl py-lg transition-colors">
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
            onClick={handleChangeWorkingDir}
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
