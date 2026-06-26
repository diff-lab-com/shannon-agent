// Multi-file batch review container (P1.1 M2).
//
// Renders a 2-pane modal: file list sidebar + current file's DiffViewer.
// Owns per-file decisions state keyed by filePath. Lazy-fetches each
// FileDiff in parallel when the modal opens. Per-file bulk operations
// plus an "Apply all" footer button that writes every file with >=1
// accepted hunk in sequence.

import { useEffect, useState, useMemo, useCallback, useRef } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import DiffViewer from '@/components/diff/DiffViewer'
import FileDiffList, { type FileFilter } from '@/components/diff/FileDiffList'
import { useModalFocus } from '@/hooks/useModalFocus'
import * as api from '@/lib/tauri-api'
import { computeHunks, mergeFile, type HunkDecision } from '@/lib/diff-merge'
import type { FileDiff } from '@/types'

interface DiffDialogMultiProps {
  open: boolean
  filePaths: string[]
  onClose: () => void
}

function cycleDecision(d: HunkDecision): HunkDecision {
  switch (d) {
    case 'pending': return 'accept'
    case 'accept': return 'reject'
    case 'reject': return 'pending'
  }
}

export default function DiffDialogMulti({ open, filePaths, onClose }: DiffDialogMultiProps) {
  const intl = useIntl()
  const [currentPath, setCurrentPath] = useState<string | null>(null)
  const [diffs, setDiffs] = useState<Map<string, FileDiff>>(new Map())
  const [loadingPaths, setLoadingPaths] = useState<Set<string>>(new Set())
  const [errors, setErrors] = useState<Map<string, string>>(new Map())
  const [decisions, setDecisions] = useState<Map<string, Map<string, HunkDecision>>>(new Map())
  const [filter, setFilter] = useState<FileFilter>('all')
  const [applying, setApplying] = useState(false)

  const containerRef = useRef<HTMLDivElement>(null)
  useModalFocus(open, containerRef)

  useEffect(() => {
    if (!open || filePaths.length === 0) {
      setCurrentPath(null)
      setDiffs(new Map())
      setLoadingPaths(new Set())
      setErrors(new Map())
      setDecisions(new Map())
      setFilter('all')
      setApplying(false)
      return
    }
    setCurrentPath(filePaths[0] ?? null)
    setDiffs(new Map())
    setLoadingPaths(new Set(filePaths))
    setErrors(new Map())
    setDecisions(new Map())
    let cancelled = false
    for (const path of filePaths) {
      api.getFileDiff(path)
        .then(d => {
          if (cancelled) return
          setDiffs(prev => new Map(prev).set(path, d))
        })
        .catch(e => {
          if (cancelled) return
          setErrors(prev => new Map(prev).set(path, e instanceof Error ? e.message : String(e)))
        })
        .finally(() => {
          if (cancelled) return
          setLoadingPaths(prev => {
            const next = new Set(prev)
            next.delete(path)
            return next
          })
        })
    }
    return () => { cancelled = true }
  }, [open, filePaths])

  useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') onClose() }
    document.addEventListener('keydown', onKey)
    return () => document.removeEventListener('keydown', onKey)
  }, [open, onClose])

  const currentDiff = currentPath ? diffs.get(currentPath) ?? null : null
  const currentError = currentPath ? errors.get(currentPath) ?? null : null
  const currentLoading = currentPath ? loadingPaths.has(currentPath) : false
  const currentDecisions = currentPath ? (decisions.get(currentPath) ?? new Map<string, HunkDecision>()) : new Map<string, HunkDecision>()

  const currentHunks = useMemo(
    () => currentDiff ? computeHunks(currentDiff.old_content, currentDiff.new_content) : [],
    [currentDiff],
  )

  const totals = useMemo(() => {
    let totalAccepted = 0
    let totalRejected = 0
    let totalPending = 0
    for (const path of filePaths) {
      const diff = diffs.get(path)
      if (!diff) continue
      const hunks = computeHunks(diff.old_content, diff.new_content)
      const fileDecisions = decisions.get(path) ?? new Map<string, HunkDecision>()
      for (const h of hunks) {
        const d = fileDecisions.get(h.id) ?? 'pending'
        if (d === 'accept') totalAccepted += 1
        else if (d === 'reject') totalRejected += 1
        else totalPending += 1
      }
    }
    return { totalAccepted, totalRejected, totalPending }
  }, [filePaths, diffs, decisions])

  const filesWithAccepts = useMemo(() => {
    const out: string[] = []
    for (const path of filePaths) {
      const fileDecisions = decisions.get(path)
      if (!fileDecisions) continue
      for (const d of fileDecisions.values()) {
        if (d === 'accept') {
          out.push(path)
          break
        }
      }
    }
    return out
  }, [filePaths, decisions])

  const handleToggleHunk = useCallback((hunkId: string) => {
    if (!currentPath) return
    setDecisions(prev => {
      const next = new Map(prev)
      const fileMap = new Map(next.get(currentPath) ?? [])
      const current = fileMap.get(hunkId) ?? 'pending'
      const cycled = cycleDecision(current)
      if (cycled === 'pending') {
        fileMap.delete(hunkId)
      } else {
        fileMap.set(hunkId, cycled)
      }
      next.set(currentPath, fileMap)
      return next
    })
  }, [currentPath])

  const setAllForCurrent = useCallback((decision: HunkDecision | 'pending') => {
    if (!currentPath || !currentDiff) return
    setDecisions(prev => {
      const next = new Map(prev)
      const hunks = computeHunks(currentDiff.old_content, currentDiff.new_content)
      if (decision === 'pending') {
        next.set(currentPath, new Map())
      } else {
        next.set(currentPath, new Map(hunks.map(h => [h.id, decision])))
      }
      return next
    })
  }, [currentPath, currentDiff])

  const handleAcceptAll = useCallback(() => setAllForCurrent('accept'), [setAllForCurrent])
  const handleRejectAll = useCallback(() => setAllForCurrent('reject'), [setAllForCurrent])
  const handleReset = useCallback(() => setAllForCurrent('pending'), [setAllForCurrent])

  const handleApplyAll = async () => {
    setApplying(true)
    let succeeded = 0
    let firstError: string | null = null
    try {
      for (const path of filesWithAccepts) {
        const diff = diffs.get(path)
        if (!diff) continue
        const fileDecisions = decisions.get(path) ?? new Map<string, HunkDecision>()
        try {
          const merged = mergeFile(diff.old_content, diff.new_content, fileDecisions)
          await api.saveTextFile(path, merged)
          succeeded += 1
        } catch (e) {
          if (!firstError) firstError = e instanceof Error ? e.message : String(e)
        }
      }
      if (firstError) {
        toast.error(
          intl.formatMessage({ id: 'diff.dialog.applyFailed' }),
          { description: firstError },
        )
      } else {
        toast.success(
          intl.formatMessage({ id: 'diff.multi.applied' }, { count: succeeded }),
          {
            description: intl.formatMessage(
              { id: 'diff.multi.applied.desc' },
              { accepted: succeeded, total: filesWithAccepts.length },
            ),
          },
        )
        onClose()
      }
    } finally {
      setApplying(false)
    }
  }

  if (!open) return null

  return (
    <div
      className="fixed inset-0 z-[200] flex items-center justify-center bg-black/30 backdrop-blur-sm p-lg"
      onClick={onClose}
    >
      <div
        ref={containerRef}
        className="bg-surface-container-lowest rounded-2xl border border-outline-variant/20 shadow-2xl w-full max-w-7xl max-h-[85vh] flex flex-col"
        role="dialog"
        aria-modal="true"
        aria-label={intl.formatMessage({ id: 'diff.multi.title' }, { count: filePaths.length })}
        onClick={e => e.stopPropagation()}
      >
        <header className="flex items-center justify-between px-lg py-md border-b border-outline-variant/30">
          <div className="flex items-center gap-md min-w-0">
            <span className="material-symbols-outlined text-[20px] text-on-surface-variant">difference</span>
            <h3 className="font-headline-md text-on-surface truncate">
              {intl.formatMessage({ id: 'diff.multi.title' }, { count: filePaths.length })}
            </h3>
            {totals.totalAccepted + totals.totalRejected + totals.totalPending > 0 && (
              <span className="font-label-sm text-on-surface-variant">
                {intl.formatMessage(
                  { id: 'diff.multi.fileCount' },
                  {
                    accepted: totals.totalAccepted,
                    rejected: totals.totalRejected,
                    pending: totals.totalPending,
                  },
                )}
              </span>
            )}
          </div>
          <Button
            className="p-xs rounded-lg hover:bg-surface-container-high text-on-surface-variant cursor-pointer"
            aria-label={intl.formatMessage({ id: 'diff.dialog.close.aria' })}
            onClick={onClose}
          >
            <span className="material-symbols-outlined">close</span>
          </Button>
        </header>

        <div className="flex-1 flex min-h-0">
          <FileDiffList
            files={filePaths}
            diffs={diffs}
            decisions={decisions}
            currentPath={currentPath}
            filter={filter}
            onSelectPath={setCurrentPath}
            onFilterChange={setFilter}
          />

          <div className="flex-1 flex flex-col min-w-0">
            {currentPath && currentHunks.length > 0 && (
              <div className="flex items-center gap-md px-lg py-sm border-b border-outline-variant/30 bg-surface-container-low">
                <div className="flex-1 min-w-0">
                  <div className="font-label-md text-on-surface truncate">{currentPath}</div>
                  <div className="font-label-sm text-on-surface-variant">
                    {intl.formatMessage({ id: 'diff.review.subtitle' })}
                  </div>
                </div>
                <div className="flex items-center gap-xs shrink-0">
                  <span className="font-label-sm text-on-surface-variant">
                    {currentDecisions.size} / {currentHunks.length}
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
                    disabled={currentDecisions.size === 0}
                    className="px-md py-xs rounded-lg font-label-md bg-surface-container-high text-on-surface-variant hover:bg-surface-container-highest cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
                  >
                    {intl.formatMessage({ id: 'diff.review.resetAll' })}
                  </button>
                </div>
              </div>
            )}

            <div className="flex-1 overflow-auto p-lg">
              {!currentPath ? (
                <p className="text-body-sm text-on-surface-variant italic">
                  {intl.formatMessage({ id: 'diff.multi.empty' })}
                </p>
              ) : currentLoading ? (
                <div className="flex items-center justify-center py-xl">
                  <span className="material-symbols-outlined animate-spin text-primary">progress_activity</span>
                  <span className="ml-md text-body-sm text-on-surface-variant">
                    {intl.formatMessage({ id: 'diff.dialog.loading' })}
                  </span>
                </div>
              ) : currentError ? (
                <div className="flex items-start gap-sm p-md bg-error/10 border border-error/20 rounded-xl text-error">
                  <span className="material-symbols-outlined text-[18px] mt-[2px]">error</span>
                  <div>
                    <p className="font-label-md">{intl.formatMessage({ id: 'diff.dialog.loadFailed' })}</p>
                    <p className="font-body-sm mt-xs opacity-80">{currentError}</p>
                  </div>
                </div>
              ) : currentDiff ? (
                <DiffViewer
                  diff={currentDiff}
                  decisions={currentDecisions}
                  onToggleHunk={handleToggleHunk}
                />
              ) : null}
            </div>
          </div>
        </div>

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
            onClick={handleApplyAll}
            disabled={applying || filesWithAccepts.length === 0}
            className="px-md py-xs rounded-lg font-label-md bg-primary text-on-primary hover:bg-primary/90 cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
            aria-label={intl.formatMessage({ id: 'diff.dialog.apply.aria' })}
          >
            {applying ? (
              <span className="flex items-center gap-xs">
                <span className="material-symbols-outlined animate-spin text-[16px]">progress_activity</span>
                {intl.formatMessage({ id: 'diff.multi.applyAll' }, { count: filesWithAccepts.length })}
              </span>
            ) : (
              intl.formatMessage({ id: 'diff.multi.applyAll' }, { count: filesWithAccepts.length })
            )}
          </button>
        </footer>
      </div>
    </div>
  )
}
