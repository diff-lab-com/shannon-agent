// Modal wrapper that fetches a FileDiff for the given path and renders
// DiffViewer with per-hunk accept/reject controls + Apply flow (P1.1 M1).
//
// Owns the decisions Map state so toggles survive re-renders. Reset
// whenever filePath changes (different file → different hunks).
//
// Apply: computes merged content client-side via mergeFile, writes via
// save_text_file, toasts success/failure, and closes the modal on success.
//
// Click outside / Esc / Close button dismisses. Errors surface as a
// friendly inline message rather than throwing.

import { useEffect, useState, useMemo } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import DiffViewer from '@/components/diff/DiffViewer'
import { useDiffKeyboard } from '@/hooks/useDiffKeyboard'
import * as api from '@/lib/tauri-api'
import { computeHunks, mergeFile, type HunkDecision } from '@/lib/diff-merge'
import type { FileDiff } from '@/types'

interface DiffDialogProps {
  open: boolean
  filePath: string | null
  onClose: () => void
}

function cycleDecision(d: HunkDecision): HunkDecision {
  switch (d) {
    case 'pending': return 'accept'
    case 'accept': return 'reject'
    case 'reject': return 'pending'
  }
}

export default function DiffDialog({ open, filePath, onClose }: DiffDialogProps) {
  const intl = useIntl()
  const [diff, setDiff] = useState<FileDiff | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [decisions, setDecisions] = useState<Map<string, HunkDecision>>(new Map())

  useEffect(() => {
    if (!open || !filePath) {
      setDiff(null)
      setError(null)
      setLoading(false)
      setDecisions(new Map())
      setApplying(false)
      return
    }
    let cancelled = false
    setLoading(true)
    setError(null)
    setDiff(null)
    setDecisions(new Map())
    setApplying(false)
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

  const hunks = useMemo(
    () => diff ? computeHunks(diff.old_content, diff.new_content) : [],
    [diff],
  )

  const decidedCount = useMemo(() => decisions.size, [decisions])
  const acceptedCount = useMemo(
    () => Array.from(decisions.values()).filter(d => d === 'accept').length,
    [decisions],
  )
  const hasHunks = hunks.length > 0
  const [applying, setApplying] = useState(false)

  const handleToggleHunk = (hunkId: string) => {
    setDecisions(prev => {
      const next = new Map(prev)
      const current = next.get(hunkId) ?? 'pending'
      const cycled = cycleDecision(current)
      if (cycled === 'pending') {
        next.delete(hunkId)
      } else {
        next.set(hunkId, cycled)
      }
      return next
    })
  }

  const handleAcceptAll = () => {
    setDecisions(new Map(hunks.map(h => [h.id, 'accept' as HunkDecision])))
  }

  const handleRejectAll = () => {
    setDecisions(new Map(hunks.map(h => [h.id, 'reject' as HunkDecision])))
  }

  const handleReset = () => {
    setDecisions(new Map())
  }

  const handleSetDecision = (hunkId: string, decision: HunkDecision) => {
    setDecisions(prev => {
      const next = new Map(prev)
      if (decision === 'pending') {
        next.delete(hunkId)
      } else {
        next.set(hunkId, decision)
      }
      return next
    })
  }

  const handleApply = async () => {
    if (!diff || !filePath) return
    setApplying(true)
    try {
      const merged = mergeFile(diff.old_content, diff.new_content, decisions)
      await api.saveTextFile(filePath, merged)
      toast.success(
        intl.formatMessage({ id: 'diff.dialog.applied' }, { count: acceptedCount }),
        { description: intl.formatMessage({ id: 'diff.dialog.applied.desc' }, { count: acceptedCount, path: filePath }) },
      )
      onClose()
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      toast.error(intl.formatMessage({ id: 'diff.dialog.applyFailed' }), { description: msg })
    } finally {
      setApplying(false)
    }
  }

  const { currentHunkId } = useDiffKeyboard({
    enabled: open && !!diff,
    hunks,
    onToggleDecision: handleSetDecision,
    onApply: acceptedCount > 0 ? handleApply : undefined,
  })
  void currentHunkId

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

        {/* Review toolbar — only render when we have a diff with hunks. */}
        {diff && hasHunks && (
          <div className="flex items-center gap-md px-lg py-sm border-b border-outline-variant/30 bg-surface-container-low">
            <div className="flex-1 min-w-0">
              <div className="font-label-md text-on-surface">{intl.formatMessage({ id: 'diff.review.title' })}</div>
              <div className="font-label-sm text-on-surface-variant">{intl.formatMessage({ id: 'diff.review.subtitle' })}</div>
            </div>
            <div className="flex items-center gap-xs shrink-0">
              <span className="font-label-sm text-on-surface-variant">
                {decidedCount} / {hunks.length}
              </span>
              <button
                type="button"
                onClick={handleAcceptAll}
                className="px-md py-xs rounded-lg font-label-md bg-tertiary-container/40 text-tertiary hover:bg-tertiary-container/60 cursor-pointer focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
              >
                {intl.formatMessage({ id: 'diff.review.acceptAll' })}
              </button>
              <button
                type="button"
                onClick={handleRejectAll}
                className="px-md py-xs rounded-lg font-label-md bg-error-container/40 text-error hover:bg-error-container/60 cursor-pointer focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
              >
                {intl.formatMessage({ id: 'diff.review.rejectAll' })}
              </button>
              <button
                type="button"
                onClick={handleReset}
                disabled={decidedCount === 0}
                className="px-md py-xs rounded-lg font-label-md bg-surface-container-high text-on-surface-variant hover:bg-surface-container-highest cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
              >
                {intl.formatMessage({ id: 'diff.review.resetAll' })}
              </button>
            </div>
          </div>
        )}

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
            <DiffViewer
              diff={diff}
              decisions={decisions}
              onToggleHunk={handleToggleHunk}
            />
          ) : null}
        </div>

        {diff && hasHunks && (
          <footer className="flex items-center justify-end gap-sm px-lg py-md border-t border-outline-variant/30 bg-surface-container-low">
            <button
              type="button"
              onClick={onClose}
              disabled={applying}
              className="px-md py-xs rounded-lg font-label-md bg-surface-container-high text-on-surface-variant hover:bg-surface-container-highest cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
            >
              {intl.formatMessage({ id: 'diff.dialog.cancel' })}
            </button>
            <button
              type="button"
              onClick={handleApply}
              disabled={applying || acceptedCount === 0}
              className="px-md py-xs rounded-lg font-label-md bg-primary text-on-primary hover:bg-primary/90 cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
              aria-label={intl.formatMessage({ id: 'diff.dialog.apply.aria' })}
            >
              {applying ? (
                <span className="flex items-center gap-xs">
                  <span className="material-symbols-outlined animate-spin text-[16px]">progress_activity</span>
                  {intl.formatMessage({ id: 'diff.dialog.apply' }, { count: acceptedCount })}
                </span>
              ) : (
                intl.formatMessage({ id: 'diff.dialog.apply' }, { count: acceptedCount })
              )}
            </button>
          </footer>
        )}
      </div>
    </div>
  )
}
