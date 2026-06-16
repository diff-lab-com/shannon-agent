// Modal wrapper that fetches a FileDiff for the given path and renders
// DiffViewer. Used by surfaces that list file paths (chat tool results,
// task detail, etc.) to satisfy the desktop P0 diff-viewer gap.
//
// Click outside / Esc / Close button dismisses. Errors surface as a
// friendly inline message rather than throwing.

import { useEffect, useState } from 'react'
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import DiffViewer from '@/components/diff/DiffViewer'
import * as api from '@/lib/tauri-api'
import type { FileDiff } from '@/types'

interface DiffDialogProps {
  open: boolean
  filePath: string | null
  onClose: () => void
}

export default function DiffDialog({ open, filePath, onClose }: DiffDialogProps) {
  const intl = useIntl()
  const [diff, setDiff] = useState<FileDiff | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!open || !filePath) {
      setDiff(null)
      setError(null)
      setLoading(false)
      return
    }
    let cancelled = false
    setLoading(true)
    setError(null)
    setDiff(null)
    api.getFileDiff(filePath)
      .then(d => { if (!cancelled) setDiff(d) })
      .catch(e => { if (!cancelled) setError(e instanceof Error ? e.message : String(e)) })
      .finally(() => { if (!cancelled) setLoading(false) })
    return () => { cancelled = true }
  }, [open, filePath])

  useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') onClose() }
    document.addEventListener('keydown', onKey)
    return () => document.removeEventListener('keydown', onKey)
  }, [open, onClose])

  if (!open) return null

  return (
    <div
      className="fixed inset-0 z-[200] flex items-center justify-center bg-black/30 backdrop-blur-sm p-lg"
      onClick={onClose}
    >
      <div
        className="bg-surface-container-lowest rounded-2xl border border-outline-variant/20 shadow-2xl w-full max-w-5xl max-h-[85vh] flex flex-col"
        role="dialog"
        aria-modal="true"
        aria-label={intl.formatMessage({ id: 'diff.dialog.file' }, { path: filePath ?? 'file' })}
        onClick={e => e.stopPropagation()}
      >
        <header className="flex items-center justify-between px-lg py-md border-b border-outline-variant/30">
          <div className="flex items-center gap-md min-w-0">
            <span className="material-symbols-outlined text-[20px] text-on-surface-variant">difference</span>
            <h3 className="font-headline-md text-on-surface truncate">{intl.formatMessage({ id: 'diff.dialog.title' })}</h3>
            {filePath ? (
              <code className="font-label-sm text-on-surface-variant bg-surface-container-low px-sm py-xs rounded truncate">{filePath}</code>
            ) : null}
          </div>
          <Button
            className="p-xs rounded-lg hover:bg-surface-container-high text-on-surface-variant cursor-pointer"
            aria-label={intl.formatMessage({ id: 'diff.dialog.close.aria' })}
            onClick={onClose}
          >
            <span className="material-symbols-outlined">close</span>
          </Button>
        </header>
        <div className="flex-1 overflow-auto p-lg">
          {loading ? (
            <div className="flex items-center justify-center py-xl">
              <span className="material-symbols-outlined animate-spin text-primary">progress_activity</span>
              <span className="ml-md text-body-sm text-on-surface-variant">{intl.formatMessage({ id: 'diff.dialog.loading' })}</span>
            </div>
          ) : error ? (
            <div className="flex items-start gap-sm p-md bg-error/10 border border-error/20 rounded-xl text-error">
              <span className="material-symbols-outlined text-[18px] mt-[2px]">error</span>
              <div>
                <p className="font-label-md">{intl.formatMessage({ id: 'diff.dialog.loadFailed' })}</p>
                <p className="font-body-sm mt-xs opacity-80">{error}</p>
              </div>
            </div>
          ) : diff ? (
            <DiffViewer diff={diff} />
          ) : null}
        </div>
      </div>
    </div>
  )
}
