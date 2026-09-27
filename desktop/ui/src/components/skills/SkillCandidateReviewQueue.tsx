// Embedded review queue for skill candidates (IA X1, 评审裁决 #2).
//
// The Extensions → Pending page is the single skill-review surface: this
// queue lists every detected candidate (D6 pattern detector — the same data
// source as the header badge and the inbox `skill_candidate` entries) with
// Save-as-skill / Reject actions. Approving goes through SkillApprovalModal
// (name/trigger edits before promotion); both actions call the
// `approve_skill_candidate` / `reject_skill_candidate` commands, which the
// backend (T5) uses to resolve the matching inbox entry — one action, one
// state, two entry points.
//
// `focusCandidateId` (router state handed over by the Triage card's
// 「去审查」) gives the matching card a one-shot ring + scroll, mirroring
// the inbox highlight pattern from IA T2.

import { useEffect, useRef, useState } from 'react'
import { useIntl, type PrimitiveType } from 'react-intl'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import ErrorState from '@/components/ui/error-state'
import { CardSkeleton } from '@/components/SkeletonLoader'
import { usePendingSkillCandidates } from '@/hooks/usePendingSkillCandidates'
import { rejectSkillCandidate } from '@/lib/tauri-api'
import type { SkillCandidate } from '@/lib/tauri-api'
import { toastError } from '@/lib/errorToast'
import { cn } from '@/lib/utils'
import { SkillApprovalModal } from '@/components/self-improve/SkillApprovalModal'

export default function SkillCandidateReviewQueue({ focusCandidateId }: {
  /** One-shot focus target (from the inbox card's 「去审查」 jump). */
  focusCandidateId?: string | null
}) {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, PrimitiveType>) => intl.formatMessage({ id }, values)
  const { candidates, loading, error, refetch } = usePendingSkillCandidates()
  const [approveTarget, setApproveTarget] = useState<SkillCandidate | null>(null)
  const [rejectingId, setRejectingId] = useState<string | null>(null)
  const listRef = useRef<HTMLDivElement>(null)

  // The modal already toasts + broadcasts `skill-catalog-changed`; here we
  // only close it and re-pull the (now shorter) queue.
  const handleApproved = () => {
    setApproveTarget(null)
    refetch()
  }

  const handleReject = async (candidate: SkillCandidate) => {
    setRejectingId(candidate.id)
    try {
      await rejectSkillCandidate(candidate.id)
      toast.success(t('skills.approval.rejected'))
      refetch()
      window.dispatchEvent(new CustomEvent('skill-catalog-changed', { detail: { source: 'pending-queue' } }))
    } catch (err) {
      toastError(t('skills.approval.rejectFailed'), err)
    } finally {
      setRejectingId(null)
    }
  }

  // One-shot focus: scroll the handed-over candidate into view. Never a
  // filter — the queue always shows everything pending.
  useEffect(() => {
    if (focusCandidateId == null || loading) return
    listRef.current?.querySelector('[data-focus-candidate="true"]')?.scrollIntoView({ block: 'center' })
  }, [focusCandidateId, loading])

  if (loading) {
    return (
      <div className="space-y-md" aria-busy="true">
        {Array.from({ length: 2 }).map((_, i) => <CardSkeleton key={i} />)}
      </div>
    )
  }

  if (error) {
    // B3 P1-17: a failed queue read used to render as "nothing pending".
    // Surface the failure (with a retry) so the two states stay distinct.
    return (
      <div className="rounded-xl border border-outline-variant/30 bg-surface-container-lowest/60">
        <ErrorState
          title={t('extensions.pending.loadFailed')}
          description={error}
          action={{ label: t('common.retry'), onClick: refetch }}
        />
      </div>
    )
  }

  if (candidates.length === 0) {
    return (
      <div className="rounded-xl border border-outline-variant/30 bg-surface-container-lowest/60 px-md py-lg text-center">
        <span className="material-symbols-outlined icon-lg text-on-surface-variant/60" aria-hidden="true">auto_awesome</span>
        <p className="font-label-md text-on-surface mt-xs">{t('extensions.pending.empty.title')}</p>
        <p className="font-body-sm text-on-surface-variant mt-xs">{t('extensions.pending.empty.description')}</p>
      </div>
    )
  }

  return (
    <>
      <div ref={listRef} role="list" aria-label={t('extensions.pending.candidates.title')} className="space-y-md">
        {candidates.map(candidate => {
          const focused = candidate.id === focusCandidateId
          const detected = new Date(candidate.detected_at)
          return (
            <div
              key={candidate.id}
              role="listitem"
              data-focus-candidate={focused ? 'true' : undefined}
              className={cn(
                'glass-panel border border-outline-variant/20 rounded-xl p-md shadow-sm bg-surface-container-lowest/80',
                focused && 'ring-2 ring-tertiary',
              )}
            >
              <div className="flex items-start gap-md">
                <div className="w-10 h-10 rounded-xl bg-surface-container-low flex items-center justify-center text-tertiary shrink-0">
                  <span className="material-symbols-outlined icon-lg" aria-hidden="true">auto_awesome</span>
                </div>
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-sm mb-xs flex-wrap">
                    <h4 className="font-body-md font-semibold text-on-surface">{candidate.proposed_name}</h4>
                    {candidate.refined && (
                      <span className="px-xs py-0.5 rounded-full bg-tertiary/10 text-tertiary font-label-sm text-[11px] font-bold uppercase tracking-wider">
                        {t('extensions.pending.candidate.refined')}
                      </span>
                    )}
                  </div>
                  <p className="text-body-sm text-on-surface-variant mb-xs break-words">{candidate.proposed_trigger}</p>
                  <ol
                    // tabIndex: keyboard-scrollable region (axe
                    // scrollable-region-focusable) — the capped-height step
                    // list must be reachable by keyboard users.
                    tabIndex={0}
                    className="list-decimal list-inside space-y-0.5 text-body-sm text-on-surface-variant bg-surface-container-low rounded-lg px-md py-sm mb-xs max-h-40 overflow-y-auto"
                  >
                    {candidate.procedure.map((step, i) => (
                      <li key={i} className="break-words">{step}</li>
                    ))}
                  </ol>
                  <div className="flex items-center gap-md flex-wrap font-label-sm text-label-sm text-on-surface-variant">
                    <span className="flex items-center gap-xs">
                      <span className="material-symbols-outlined text-[14px]" aria-hidden="true">history</span>
                      {t('extensions.pending.candidate.occurrences', { count: candidate.occurrence_count })}
                    </span>
                    <span className="flex items-center gap-xs">
                      <span className="material-symbols-outlined text-[14px]" aria-hidden="true">schedule</span>
                      {t('extensions.pending.candidate.detectedAt', { date: detected.toLocaleString(intl.locale) })}
                    </span>
                    {candidate.example_session_ids.length > 0 && (
                      <span className="flex items-center gap-xs">
                        <span className="material-symbols-outlined text-[14px]" aria-hidden="true">forum</span>
                        {t('extensions.pending.candidate.sessions', { count: candidate.example_session_ids.length })}
                      </span>
                    )}
                  </div>
                </div>
                <div className="flex items-center gap-sm shrink-0">
                  <Button
                    size="sm"
                    variant="ghost"
                    disabled={rejectingId === candidate.id}
                    aria-label={t('extensions.pending.candidate.rejectAria', { name: candidate.proposed_name })}
                    className="cursor-pointer inline-flex items-center gap-xs text-on-surface-variant hover:text-error"
                    onClick={() => void handleReject(candidate)}
                  >
                    <span className="material-symbols-outlined text-[16px]" aria-hidden="true">close</span>
                    {t('skills.approval.reject')}
                  </Button>
                  <Button
                    size="sm"
                    aria-label={t('extensions.pending.candidate.approveAria', { name: candidate.proposed_name })}
                    className="cursor-pointer inline-flex items-center gap-xs"
                    onClick={() => setApproveTarget(candidate)}
                  >
                    <span className="material-symbols-outlined text-[16px]" aria-hidden="true">save</span>
                    {t('skills.approval.approve')}
                  </Button>
                </div>
              </div>
            </div>
          )
        })}
      </div>
      <SkillApprovalModal
        open={approveTarget != null}
        candidate={approveTarget}
        onClose={() => setApproveTarget(null)}
        onApproved={handleApproved}
      />
    </>
  )
}
