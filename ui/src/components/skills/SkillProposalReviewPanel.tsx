// Modal panel for reviewing skill proposals.
//
// Displays proposal details (name, description, triggers, workflow) with
// Approve/Reject buttons. Fetches proposals on mount and refreshes after actions.

import { useEffect, useState } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { skillLoop } from '@/lib/tauri-api'
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
  const t = (id: string) => intl.formatMessage({ id })
  const [proposals, setProposals] = useState<SkillProposal[]>([])
  const [currentIndex, setCurrentIndex] = useState(0)
  const [loading, setLoading] = useState(false)
  const [actionLoading, setActionLoading] = useState(false)

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
          console.error('Failed to load proposals:', err)
          toast.error(t('skillProposals.review.loadError'))
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
        className="bg-white dark:bg-gray-800 rounded-lg shadow-xl max-w-2xl w-full max-h-[80vh] overflow-hidden flex flex-col"
        role="dialog"
        aria-modal="true"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-center justify-between p-6 border-b dark:border-gray-700">
          <h2 className="text-xl font-semibold text-gray-900 dark:text-gray-100">
            {t('skillProposals.review.title')}
          </h2>
          <button
            onClick={onClose}
            className="text-gray-400 hover:text-gray-600 dark:hover:text-gray-300"
            aria-label="Close"
          >
            <span className="material-symbols-outlined">close</span>
          </button>
        </div>

        {/* Content */}
        <div className="flex-1 overflow-y-auto p-6">
          {loading ? (
            <div className="flex items-center justify-center py-12">
              <span className="material-symbols-outlined text-[32px] text-primary animate-spin">
                progress_activity
              </span>
            </div>
          ) : !current ? (
            <div className="text-center py-12">
              <p className="text-gray-600 dark:text-gray-400">
                {t('skillProposals.review.empty')}
              </p>
            </div>
          ) : (
            <div className="space-y-6">
              {/* Name */}
              <div>
                <h3 className="text-sm font-medium text-gray-500 dark:text-gray-400 mb-1">
                  {t('skillProposals.review.card.name')}
                </h3>
                <p className="text-lg font-semibold text-gray-900 dark:text-gray-100">
                  {current.name}
                </p>
              </div>

              {/* Description */}
              <div>
                <h3 className="text-sm font-medium text-gray-500 dark:text-gray-400 mb-1">
                  {t('skillProposals.review.card.description')}
                </h3>
                <p className="text-gray-700 dark:text-gray-300">
                  {current.description}
                </p>
              </div>

              {/* Trigger Patterns */}
              <div>
                <h3 className="text-sm font-medium text-gray-500 dark:text-gray-400 mb-2">
                  {t('skillProposals.review.card.triggers')}
                </h3>
                <div className="flex flex-wrap gap-2">
                  {current.trigger_patterns.map((pattern, i) => (
                    <span
                      key={i}
                      className="px-2 py-1 bg-blue-50 dark:bg-blue-900/20 text-blue-700 dark:text-blue-300 text-xs rounded-md"
                    >
                      {pattern}
                    </span>
                  ))}
                </div>
              </div>

              {/* Example Workflow */}
              <div>
                <h3 className="text-sm font-medium text-gray-500 dark:text-gray-400 mb-1">
                  {t('skillProposals.review.card.example')}
                </h3>
                <pre className="mt-1 p-3 bg-gray-50 dark:bg-gray-900 rounded text-sm text-gray-800 dark:text-gray-200 whitespace-pre-wrap overflow-x-auto">
                  {current.example_workflow}
                </pre>
              </div>

              {/* Created At */}
              <div className="text-xs text-gray-500 dark:text-gray-400">
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
              <div className="flex items-center justify-center gap-2 px-6 py-3 border-t dark:border-gray-700">
                <button
                  onClick={handlePrevious}
                  disabled={actionLoading}
                  className="px-3 py-1.5 text-sm text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-md disabled:opacity-50 transition-colors"
                >
                  {t('skillProposals.review.previous')}
                </button>
                <span className="text-sm text-gray-500">
                  {currentIndex + 1} / {proposals.length}
                </span>
                <button
                  onClick={handleNext}
                  disabled={actionLoading}
                  className="px-3 py-1.5 text-sm text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-md disabled:opacity-50 transition-colors"
                >
                  {t('skillProposals.review.next')}
                </button>
              </div>
            )}

            {/* Actions */}
            <div className="flex justify-end gap-3 p-6 border-t dark:border-gray-700">
              <button
                onClick={handleReject}
                disabled={actionLoading}
                className="px-4 py-2 text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-md disabled:opacity-50 transition-colors"
              >
                {t('skillProposals.review.rejectButton')}
              </button>
              <button
                onClick={handleApprove}
                disabled={actionLoading}
                className="px-4 py-2 bg-blue-600 hover:bg-blue-700 text-white rounded-md disabled:opacity-50 transition-colors"
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
