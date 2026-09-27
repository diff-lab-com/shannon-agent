// Shared single-file diff review body — fetch + per-hunk accept/reject
// decisions + Apply flow (P1.1 M1), extracted from DiffDialog so the right
// dock's Diff tab (ZCode delta ⑦) can host the same review surface
// side-by-side with the conversation instead of blocking it behind a modal.
//
// Owns the decisions Map state so toggles survive re-renders; state resets
// whenever filePath changes (different file → different hunks) or the body
// unmounts. Apply computes merged content client-side via mergeFile, writes
// via save_text_file, toasts success/failure, and calls onClose on success.

import { useEffect, useState, useMemo, useRef } from 'react'
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

/** B0 P0-3: binary / non-UTF-8 reads arrive as `{ code, message }`. */
interface StructuredIpcError {
  code?: string
  message?: string
}

function asStructuredError(e: unknown): StructuredIpcError | null {
  if (e && typeof e === 'object' && typeof (e as StructuredIpcError).code === 'string') {
    return e as StructuredIpcError
  }
  return null
}

export default function DiffReviewBody({ filePath, onClose, active }: DiffReviewBodyProps) {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, string | number>) =>
    intl.formatMessage({ id }, values)
  const [diff, setDiff] = useState<FileDiff | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  // B0 P0-3: set when the fetch failed with a structured binary/UTF-8 error
  // so the message renders through i18n instead of the raw IPC string.
  const [errorCode, setErrorCode] = useState<'binary' | 'utf8' | null>(null)
  const [decisions, setDecisions] = useState<Map<string, HunkDecision>>(new Map())
  const [applying, setApplying] = useState(false)
  // B0 P0-4 [R4-1]: same-tick double invocations (Enter twice before React
  // re-renders) share the stale `applying` state — the ref closes that hole.
  const applyingRef = useRef(false)
  // B0 P0-4: keyboard shortcuts only fire while focus is inside this node.
  const containerRef = useRef<HTMLDivElement | null>(null)

  useEffect(() => {
    if (!active || !filePath) {
      setDiff(null)
      setError(null)
      setErrorCode(null)
      setLoading(false)
      setDecisions(new Map())
      setApplying(false)
      applyingRef.current = false
      return
    }
    let cancelled = false
    setLoading(true)
    setError(null)
    setErrorCode(null)
    setDiff(null)
    setDecisions(new Map())
    setApplying(false)
    applyingRef.current = false
    api.getFileDiff(filePath)
      .then(d => { if (!cancelled) setDiff(d) })
      .catch(e => {
        if (cancelled) return
        const structured = asStructuredError(e)
        // B0 P0-3: binary / non-UTF-8 reads arrive as `{ code, message }`.
        if (structured?.code === 'binary_file' || structured?.code === 'not_utf8') {
          setErrorCode(structured.code === 'binary_file' ? 'binary' : 'utf8')
        } else {
          setError(e instanceof Error ? e.message : String(e))
        }
      })
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
  // B0 P0-3: accepting this diff would blank a non-empty file. Block Apply
  // (the backend binary guard already refuses to produce such a diff).
  const wholeFileDeletion = !!diff
    && diff.old_content.trim().length > 0
    && diff.new_content.trim().length === 0

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
    // B0 P0-4 [R4-1]: re-entry guard of last resort — the keyboard layer
    // dedupes too, but Apply must never run twice even if invoked directly
    // (double-click within one render tick, double Enter, …).
    if (applyingRef.current) return
    if (!diff || !filePath || wholeFileDeletion) return
    applyingRef.current = true
    setApplying(true)
    try {
      const merged = mergeFile(diff.old_content, diff.new_content, decisions)
      // B0 P0-3: send the fetch-time mtime — the backend rejects the write
      // with `{ code: 'mtime_conflict' }` if the file changed meanwhile.
      await api.saveTextFile(filePath, merged, diff.mtime)
      toast.success(
        t('diff.dialog.applied'),
        { description: t('diff.dialog.applied.desc') },
      )
      onClose()
    } catch (e) {
      if (asStructuredError(e)?.code === 'mtime_conflict') {
        toast.error(t('diff.dialog.conflict'), {
          description: t('diff.dialog.conflict.desc'),
        })
      } else {
        const msg = e instanceof Error ? e.message : String(e)
        toast.error(t('diff.dialog.applyFailed'), { description: msg })
      }
    } finally {
      applyingRef.current = false
      setApplying(false)
    }
  }

  // B4 P1-33: the keyboard cursor is now visual — DiffViewer rings the
  // current hunk's header and scrolls it into view (previously the id was
  // computed and discarded with `void`).
  const { currentHunkId } = useDiffKeyboard({
    enabled: active && !!diff,
    hunks,
    containerRef,
    applying,
    onToggleDecision: handleSetDecision,
    onApply: acceptedCount > 0 && !wholeFileDeletion ? handleApply : undefined,
  })

  return (
    <div ref={containerRef} className="flex flex-col flex-1 min-h-0">
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
        ) : error || errorCode ? (
          <div className="flex items-start gap-sm p-md bg-error/10 border border-error/20 rounded-xl text-error">
            <span className="material-symbols-outlined text-[18px] mt-[2px]" aria-hidden="true">error</span>
            <div>
              <p className="font-label-md">{t('diff.dialog.loadFailed')}</p>
              <p className="font-body-sm mt-xs opacity-80">
                {errorCode === 'binary' || errorCode === 'utf8'
                  ? t('diff.dialog.binaryFile')
                  : error}
              </p>
            </div>
          </div>
        ) : diff ? (
          <>
            {wholeFileDeletion && (
              <div
                role="alert"
                className="flex items-start gap-sm p-md mb-md bg-error/10 border border-error/30 rounded-xl text-error"
              >
                <span className="material-symbols-outlined text-[18px] mt-[2px]" aria-hidden="true">warning</span>
                <p className="font-label-md">{t('diff.review.deleteWarning')}</p>
              </div>
            )}
            <DiffViewer
              diff={diff}
              decisions={decisions}
              onToggleHunk={handleToggleHunk}
              currentHunkId={currentHunkId}
            />
          </>
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
            disabled={applying || acceptedCount === 0 || wholeFileDeletion}
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
