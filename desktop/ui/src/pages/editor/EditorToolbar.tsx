// Editor toolbar — file chip + language id + diagnostic count + re-run button
// + edit-mode toggle (Save / Cancel while editing). Stateless: every prop
// and callback is owned by the Editor orchestrator.

import { Button } from '@/components/ui/button'
import { Spinner } from '@/components/ui/loading-state'
import type { SourceFile } from '@/lib/tauri-api'

interface EditorToolbarProps {
  t: (id: string, values?: Record<string, string | number | boolean>) => string
  file: SourceFile
  diagCount: number
  diagLoading: boolean
  editMode: boolean
  saving: boolean
  fetchDiagnostics: (sourceFile: SourceFile) => Promise<void>
  onToggleEdit: () => void
  onSave: () => Promise<void>
}

export default function EditorToolbar({
  t,
  file,
  diagCount,
  diagLoading,
  editMode,
  saving,
  fetchDiagnostics,
  onToggleEdit,
  onSave,
}: EditorToolbarProps) {
  return (
    <div className="flex items-center gap-sm font-label-sm text-on-surface-variant flex-wrap">
      <code className="font-mono bg-surface-container-low px-1.5 py-0.5 rounded">
        {file.path.split('/').pop()}
      </code>
      <span className="text-[11px] uppercase tracking-wider">
        {file.language_id}
      </span>
      <span>·</span>
      <span>{diagCount} {t(`editor.diagnostics`, { count: diagCount })}</span>
      <Button
        type="button"
        variant="outline"
        onClick={() => void fetchDiagnostics(file)}
        disabled={diagLoading}
        className="flex items-center gap-xs px-sm py-0.5 rounded-full border border-outline-variant/40 bg-surface-container-low text-on-surface hover:bg-surface-container-high focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 disabled:opacity-40 disabled:cursor-not-allowed cursor-pointer"
        aria-label={t('editor.reRun')}
      >
        {diagLoading ? (
          <Spinner className="text-[14px]" />
        ) : (
          <span className="text-[14px]">refresh</span>
        )}
        <span>{diagLoading ? t('editor.running') : t('editor.reRun')}</span>
      </Button>
      <div className="ml-auto flex items-center gap-xs">
        {editMode ? (
          <>
            <Button
              type="button"
              onClick={onSave}
              disabled={saving}
              className="flex items-center gap-xs px-sm py-0.5 rounded-full bg-primary text-on-primary hover:bg-primary/90 focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 disabled:opacity-40 disabled:cursor-not-allowed cursor-pointer"
            >
              <span className="material-symbols-outlined text-[14px]">
                {saving ? 'progress_activity' : 'save'}
              </span>
              <span>{saving ? t('editor.saving') : t('editor.save')}</span>
            </Button>
            <Button
              type="button"
              variant="outline"
              onClick={onToggleEdit}
              disabled={saving}
              className="flex items-center gap-xs px-sm py-0.5 rounded-full border border-outline-variant/40 bg-surface-container-low text-on-surface hover:bg-surface-container-high focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 disabled:opacity-40 disabled:cursor-not-allowed cursor-pointer"
            >
              <span className="material-symbols-outlined text-[14px]">
                close
              </span>
              <span>{t('editor.cancel')}</span>
            </Button>
          </>
        ) : (
          <Button
            type="button"
            variant="outline"
            onClick={onToggleEdit}
            className="flex items-center gap-xs px-sm py-0.5 rounded-full border border-outline-variant/40 bg-surface-container-low text-on-surface hover:bg-surface-container-high focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 cursor-pointer"
            aria-label={t('editor.editMode')}
          >
            <span className="material-symbols-outlined text-[14px]">edit</span>
            <span>{t('editor.editMode')}</span>
          </Button>
        )}
      </div>
    </div>
  )
}