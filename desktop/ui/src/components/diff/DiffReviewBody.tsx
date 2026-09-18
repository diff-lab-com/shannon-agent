// Shared single-file diff review body — fetch + per-hunk accept/reject
// decisions + Apply flow (P1.1 M1), extracted from DiffDialog so the right
// dock's Diff tab (ZCode delta ⑦) can host the same review surface
// side-by-side with the conversation instead of blocking it behind a modal.
//
// Owns the decisions Map state so toggles survive re-renders; state resets
// whenever filePath changes (different file → different hunks) or the body
// unmounts. Apply computes merged content client-side via mergeFile, writes
// via save_text_file, toasts success/failure, and calls onClose on success.

import { useEffect, useState, useMemo } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { Spinner } from '@/components/ui/loading-state'
import { Button } from '@/components/ui/button'
import DiffViewer from '@/components/diff/DiffViewer'
import { useDiffKeyboard } from '@/hooks/useDiffKeyboard'
import * as api from '@/lib/tauri-api'
import { computeHunks, mergeFile, type HunkDecision } from '@/lib/diff-merge'
import type { FileDiff } from '@/types'

interface DiffReviewBodyProps {
  filePath: string | null
  onClose: () => void
  /** Whether the host surface is live (modal open / dock tab active). */
  active: boolean
}

function cycleDecision(d: HunkDecision): HunkDecision {
  switch (d) {
    case 'pending': return 'accept'
    case 'accept': return 'reject'
    case 'reject': return 'pending'
  }
}

export default function DiffReviewBody({ filePath, onClose, active }: DiffReviewBodyProps) {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, string | number>) =>
    intl.formatMessage({ id }, values)
  const [diff, setDiff] = useState<FileDiff | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [decisions, setDecisions] = useState<Map<string, HunkDecision>>(new Map())
  const [applying, setApplying] = useState(false)

  useEffect(() => {
    if (!active || !filePath) {
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
  }, [active, filePath])

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
        t('diff.dialog.applied'),
        { description: t('diff.dialog.applied.desc') },
      )
      onClose()
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      toast.error(t('diff.dialog.applyFailed'), { description: msg })
    } finally {
      setApplying(false)
    }
  }

  const { currentHunkId } = useDiffKeyboard({
    enabled: active && !!diff,
    hunks,
    onToggleDecision: handleSetDecision,
    onApply: acceptedCount > 0 ? handleApply : undefined,
  })
  void currentHunkId

  return (
    <div className="flex flex-col flex-1 min-h-0">
      {diff && hasHunks && (
        <div className="flex flex-wrap items-center gap-md px-lg py-sm border-b border-outline-variant/30 bg-surface-container-low">
          <div className="flex-1 min-w-0">
            <div className="font-label-md text-on-surface">{t('diff.review.title')}</div>
            <div className="font-label-sm text-on-surface-variant">{t('diff.review.subtitle')}</div>
          </div>
          <div className="flex items-center gap-xs shrink-0">
            <span className="font-label-sm text-on-surface-variant">
              {decidedCount} / {hunks.length}
            </span>
            <Button
              size="sm"
              onClick={handleAcceptAll}
              className="h-auto px-md py-xs rounded-lg font-label-md bg-tertiary-container/40 text-tertiary hover:bg-tertiary-container/60"
            >
              {t('diff.review.acceptAll')}
            </Button>
            <Button
              size="sm"
              onClick={handleRejectAll}
              className="h-auto px-md py-xs rounded-lg font-label-md bg-error-container/40 text-error hover:bg-error-container/60"
            >
              {t('diff.review.rejectAll')}
            </Button>
            <Button
              size="sm"
              variant="secondary"
              onClick={handleReset}
              disabled={decidedCount === 0}
              className="h-auto px-md py-xs rounded-lg font-label-md"
            >
              {t('diff.review.resetAll')}
            </Button>
          </div>
        </div>
      )}

      <div className="flex-1 overflow-auto p-lg">
        {loading ? (
          <div className="flex items-center justify-center py-xl">
            <Spinner className="text-primary" />
            <span className="ml-md text-body-sm text-on-surface-variant">{t('diff.dialog.loading')}</span>
          </div>
        ) : error ? (
          <div className="flex items-start gap-sm p-md bg-error/10 border border-error/20 rounded-xl text-error">
            <span className="material-symbols-outlined text-[18px] mt-[2px]" aria-hidden="true">error</span>
            <div>
              <p className="font-label-md">{t('diff.dialog.loadFailed')}</p>
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
        <div className="flex items-center justify-end gap-sm px-lg py-sm border-t border-outline-variant/20 pt-md">
          <Button
            variant="secondary"
            size="sm"
            onClick={onClose}
            disabled={applying}
            className="h-auto px-md py-xs rounded-lg font-label-md"
          >
            {t('diff.dialog.cancel')}
          </Button>
          <Button
            size="sm"
            onClick={handleApply}
            disabled={applying || acceptedCount === 0}
            className="h-auto px-md py-xs rounded-lg font-label-md bg-primary text-on-primary hover:bg-primary/90"
            aria-label={t('diff.dialog.apply.aria')}
          >
            {applying ? (
              <span className="flex items-center gap-xs">
                <Spinner className="text-[16px]" />
                {t('diff.dialog.apply', { count: acceptedCount })}
              </span>
            ) : (
              t('diff.dialog.apply', { count: acceptedCount })
            )}
          </Button>
        </div>
      )}
    </div>
  )
}
