// File load form — file path text input + Browse button + Load button + load
// error banner. Pure presentational; all state + callbacks come from the
// orchestrator.

import { Button } from '@/components/ui/button'

interface FileLoadFormProps {
  t: (id: string, values?: Record<string, string | number | boolean>) => string
  filePath: string
  setFilePath: (s: string) => void
  loading: boolean
  loadError: string | null
  onLoad: (e: React.FormEvent) => Promise<void>
  onBrowse: () => Promise<void>
}

export default function FileLoadForm({
  t,
  filePath,
  setFilePath,
  loading,
  loadError,
  onLoad,
  onBrowse,
}: FileLoadFormProps) {
  return (
    <form
      onSubmit={onLoad}
      className="bg-surface-container-lowest rounded-2xl p-md border border-outline-variant/30 shadow-sm flex flex-col gap-sm"
    >
      <label className="font-label-sm text-on-surface-variant flex flex-col gap-xs">
        {t('editor.filePath')}
        <div className="flex gap-xs">
          <input
            type="text"
            value={filePath}
            onChange={(e) => setFilePath(e.target.value)}
            placeholder={t('editor.filePath.placeholder')}
            className="flex-1 font-mono font-label-md bg-surface-container-low text-on-surface border border-outline-variant/40 rounded-lg px-sm py-xs focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
          />
          <Button
            type="button"
            variant="outline"
            onClick={onBrowse}
            aria-label={t('editor.browse')}
            className="flex items-center gap-xs px-md py-xs rounded-lg border border-outline-variant/40 bg-surface-container-low text-on-surface hover:bg-surface-container-high focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 cursor-pointer"
          >
            <span className="material-symbols-outlined text-[18px]">
              folder_open
            </span>
            <span className="font-label-md">{t('editor.browse')}</span>
          </Button>
        </div>
      </label>
      <Button
        type="submit"
        disabled={!filePath.trim() || loading}
        className="self-start font-label-md bg-primary text-on-primary rounded-lg px-md py-sm cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed hover:bg-primary/90 focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
      >
        {loading ? t('editor.loading') : t('editor.loadFile')}
      </Button>
      {loadError ? (
        <div
          className="bg-error/10 border border-error/30 rounded-lg p-sm font-label-sm text-error"
          role="alert"
        >
          {loadError}
        </div>
      ) : null}
    </form>
  )
}