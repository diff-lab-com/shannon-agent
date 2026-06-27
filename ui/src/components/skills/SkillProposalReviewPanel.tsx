// Modal panel for reviewing skill proposals.
//
// Displays proposal details (name, description, triggers, workflow) with
// Approve/Reject buttons. Fetches proposals on mount and refreshes after actions.

import { useCallback, useEffect, useRef, useState } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { toastError } from '@/lib/errorToast'
import { skillLoop } from '@/lib/tauri-api'
import { useModalFocus } from '@/hooks/useModalFocus'
import type { SkillProposal } from '@/types'

interface SkillProposalReviewPanelProps {
  open: boolean
  onClose: () => void
}

export default function SkillProposalReviewPanel({
  open,
  onClose,
}: SkillProposalReviewPanelProps) {
  const intl = useIntl()
  const t = useCallback((id: string) => intl.formatMessage({ id }), [intl])
  const [proposals, setProposals] = useState<SkillProposal[]>([])
  const [currentIndex, setCurrentIndex] = useState(0)
  const [loading, setLoading] = useState(false)
  const [actionLoading, setActionLoading] = useState(false)

  const containerRef = useRef<HTMLDivElement>(null)
  useModalFocus(open, containerRef)

  useEffect(() => {
    if (!open) return

    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
    }
    document.addEventListener('keydown', onKey)
    return () => document.removeEventListener('keydown', onKey)
  }, [open, onClose])

  useEffect(() => {
    if (!open) {
      setProposals([])
      setCurrentIndex(0)
      return
    }

    let cancelled = false
    setLoading(true)
    skillLoop
      .listProposals()
      .then((data) => {
        if (!cancelled) {
          setProposals(data)
          setCurrentIndex(0)
        }
      })
      .catch((err) => {
        if (!cancelled) {
          toastError(t('skillProposals.review.loadError'), err)
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false)
      })

    return () => {
      cancelled = true
    }
  }, [open, t])

  const current = proposals[currentIndex]

  const handleApprove = async () => {
    if (!current) return
    setActionLoading(true)
    try {
      await skillLoop.approve(current.id)
      toast.success(t('skillProposals.review.approved'))
      // Remove current proposal and move to next
      setProposals((prev) => prev.filter((p) => p.id !== current.id))
      if (currentIndex >= proposals.length - 1) {
        setCurrentIndex(0)
      }
      if (proposals.length === 1) {
        onClose()
      }
    } catch (err) {
      console.error('Failed to approve proposal:', err)
      const msg = err instanceof Error ? err.message : String(err)
      if (msg.includes('Similar skill')) {
        toast.error(t('skillProposals.review.duplicateError'))
      } else {
        toast.error(msg)
      }
    } finally {
      setActionLoading(false)
    }
  }

  const handleReject = async () => {
    if (!current) return
    setActionLoading(true)
    try {
      await skillLoop.reject(current.id)
      toast.success(t('skillProposals.review.rejected'))
      // Remove current proposal and move to next
      setProposals((prev) => prev.filter((p) => p.id !== current.id))
      if (currentIndex >= proposals.length - 1) {
        setCurrentIndex(0)
      }
      if (proposals.length === 1) {
        onClose()
      }
    } catch (err) {
      console.error('Failed to reject proposal:', err)
      toast.error(err instanceof Error ? err.message : String(err))
    } finally {
      setActionLoading(false)
    }
  }

  const handlePrevious = () => {
    setCurrentIndex((prev) => (prev > 0 ? prev - 1 : proposals.length - 1))
  }

  const handleNext = () => {
    setCurrentIndex((prev) => (prev < proposals.length - 1 ? prev + 1 : 0))
  }

  if (!open) return null

  return (
    <div
      className="fixed inset-0 z-[200] flex items-center justify-center bg-black/30 backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        ref={containerRef}
        className="bg-surface-container-lowest rounded-lg shadow-xl max-w-2xl w-full max-h-[80vh] overflow-hidden flex flex-col border border-outline-variant/30"
        role="dialog"
        aria-modal="true"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-center justify-between p-6 border-b border-outline-variant">
          <h2 className="text-xl font-semibold text-on-surface">
            {t('skillProposals.review.title')}
          </h2>
          <button
            onClick={onClose}
            className="text-on-surface-variant hover:text-on-surface"
            aria-label={t('skillProposals.review.closeAria')}
          >
            <span className="material-symbols-outlined">close</span>
          </button>
        </div>

        {/* Content */}
        <div className="flex-1 overflow-y-auto p-6">
          {loading ? (
            <div className="flex items-center justify-center py-12">
              <span className="material-symbols-outlined icon-xl text-primary animate-spin">
                progress_activity
              </span>
            </div>
          ) : !current ? (
            <div className="text-center py-12">
              <p className="text-on-surface-variant">
                {t('skillProposals.review.empty')}
              </p>
            </div>
          ) : (
            <div className="space-y-6">
              {/* Name */}
              <div>
                <h3 className="text-sm font-medium text-on-surface-variant mb-1">
                  {t('skillProposals.review.card.name')}
                </h3>
                <p className="text-lg font-semibold text-on-surface">
                  {current.name}
                </p>
              </div>

              {/* Description */}
              <div>
                <h3 className="text-sm font-medium text-on-surface-variant mb-1">
                  {t('skillProposals.review.card.description')}
                </h3>
                <p className="text-on-surface">
                  {current.description}
                </p>
              </div>

              {/* Trigger Patterns */}
              <div>
                <h3 className="text-sm font-medium text-on-surface-variant mb-2">
                  {t('skillProposals.review.card.triggers')}
                </h3>
                <div className="flex flex-wrap gap-2">
                  {current.trigger_patterns.map((pattern, i) => (
                    <span
                      key={i}
                      className="px-2 py-1 bg-primary/10 text-primary text-xs rounded-md"
                    >
                      {pattern}
                    </span>
                  ))}
                </div>
              </div>

              {/* Example Workflow */}
              <div>
                <h3 className="text-sm font-medium text-on-surface-variant mb-1">
                  {t('skillProposals.review.card.example')}
                </h3>
                <pre className="mt-1 p-3 bg-surface-container-low rounded text-sm text-on-surface whitespace-pre-wrap overflow-x-auto">
                  {current.example_workflow}
                </pre>
              </div>

              {/* Created At */}
              <div className="text-xs text-on-surface-variant">
                {intl.formatMessage(
                  { id: 'skillProposals.review.card.created' },
                  {
                    date: new Date(current.created_at).toLocaleString(
                      intl.locale
                    ),
                  }
                )}
              </div>
            </div>
          )}
        </div>

        {/* Footer */}
        {current && (
          <>
            {/* Navigation */}
            {proposals.length > 1 && (
              <div className="flex items-center justify-center gap-2 px-6 py-3 border-t border-outline-variant">
                <button
                  onClick={handlePrevious}
                  disabled={actionLoading}
                  className="px-3 py-1.5 text-sm text-on-surface-variant hover:bg-surface-container rounded-md disabled:opacity-50 transition-colors"
                >
                  {t('skillProposals.review.previous')}
                </button>
                <span className="text-sm text-on-surface-variant">
                  {currentIndex + 1} / {proposals.length}
                </span>
                <button
                  onClick={handleNext}
                  disabled={actionLoading}
                  className="px-3 py-1.5 text-sm text-on-surface-variant hover:bg-surface-container rounded-md disabled:opacity-50 transition-colors"
                >
                  {t('skillProposals.review.next')}
                </button>
              </div>
            )}

            {/* Actions */}
            <div className="flex justify-end gap-3 p-6 border-t border-outline-variant">
              <button
                onClick={handleReject}
                disabled={actionLoading}
                className="px-4 py-2 text-on-surface hover:bg-surface-container rounded-md disabled:opacity-50 transition-colors"
              >
                {t('skillProposals.review.rejectButton')}
              </button>
              <button
                onClick={handleApprove}
                disabled={actionLoading}
                className="px-4 py-2 bg-primary hover:bg-primary/90 text-on-primary rounded-md disabled:opacity-50 transition-colors"
              >
                {actionLoading
                  ? t('skillProposals.review.approving')
                  : t('skillProposals.review.approveButton')}
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  )
}
