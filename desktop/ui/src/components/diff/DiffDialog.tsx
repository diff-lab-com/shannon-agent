// Modal wrapper that fetches a FileDiff for the given path and renders
// DiffViewer with per-hunk accept/reject controls + Apply flow (P1.1 M1).
//
// Owns the decisions Map state so toggles survive re-renders. Reset
// whenever filePath changes (different file → different hunks).
//
// Apply: computes merged content client-side via mergeFile, writes via
// save_text_file, toasts success/failure, and closes the modal on success.
//
// T1.2 — migrated onto the shared Modal primitive. Focus management,
// Esc-to-close, body scroll lock, and backdrop click are now handled
// by the primitive instead of hand-rolled hooks/listeners. The close
// button's aria-label is supplied via Modal's `closeLabel` prop so the
// existing test contract ("Close diff") keeps working.

import { useEffect, useState, useMemo } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Modal, ModalBody, ModalFooter } from '@/components/ui/modal'
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
  const t = (id: string, values?: Record<string, string | number>) =>
    intl.formatMessage({ id }, values)
  const [diff, setDiff] = useState<FileDiff | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [decisions, setDecisions] = useState<Map<string, HunkDecision>>(new Map())
  const [applying, setApplying] = useState(false)

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
    enabled: open && !!diff,
    hunks,
    onToggleDecision: handleSetDecision,
    onApply: acceptedCount > 0 ? handleApply : undefined,
  })
  void currentHunkId

  return (
    <Modal
      open={open}
      onClose={onClose}
      size="2xl"
      title={t('diff.dialog.title')}
      description={filePath ?? undefined}
      closeLabel={t('diff.dialog.close.aria')}
      busy={applying}
      className="max-w-5xl flex flex-col max-h-[90vh]"
    >
      {diff && hasHunks && (
        <div className="flex items-center gap-md px-lg py-sm border-b border-outline-variant/30 bg-surface-container-low">
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

      <ModalBody className="flex-1 overflow-auto p-lg">
        {loading ? (
          <div className="flex items-center justify-center py-xl">
            <span className="material-symbols-outlined animate-spin text-primary" aria-hidden="true">progress_activity</span>
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
      </ModalBody>

      {diff && hasHunks && (
        <ModalFooter className="pt-md">
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
                <span className="material-symbols-outlined animate-spin text-[16px]" aria-hidden="true">progress_activity</span>
                {t('diff.dialog.apply', { count: acceptedCount })}
              </span>
            ) : (
              t('diff.dialog.apply', { count: acceptedCount })
            )}
          </Button>
        </ModalFooter>
      )}
    </Modal>
  )
}