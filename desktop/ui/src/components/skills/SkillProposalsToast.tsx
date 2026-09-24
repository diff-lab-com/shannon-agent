// Light "pending review" notice for skill proposals (IA X1).
//
// Degraded from the old toast + big review panel: the toast no longer opens
// the review UI — it is a single lightweight nudge that navigates to
// /extensions/pending, the single skill-review surface (评审裁决 #2).
//
// Two count sources, one nudge:
//  - skill candidates (pattern detector) via usePendingSkillCandidates —
//    the same source of truth as the Extensions tab badge and header bell;
//  - skill-loop proposal drafts via the backend's `skill-proposal-available`
//    event (X1 fix: fired on draft creation and with the updated count after
//    approve/reject elsewhere).
// Suppressed on /extensions/pending itself — the queue updates in place
// there, so nudging the user to the page they're on is noise.

import { useEffect, useState } from 'react'
import { useIntl } from 'react-intl'
import { useLocation, useNavigate } from 'react-router-dom'
import { Button } from '@/components/ui/button'
import { usePendingSkillCandidates } from '@/hooks/usePendingSkillCandidates'
import { useTauriEventValidated } from '@/hooks/useTauriEventValidated'
import type { SkillProposalCountPayload } from '@/types'

export default function SkillProposalsToast() {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const navigate = useNavigate()
  const location = useLocation()
  const { candidates, refetch } = usePendingSkillCandidates()
  const [draftCount, setDraftCount] = useState(0)
  const [dismissed, setDismissed] = useState(false)

  // Draft arrival channel (X1 fix): the payload carries the updated total
  // of pending proposal drafts, not a delta.
  useTauriEventValidated<SkillProposalCountPayload>('skill-proposal-available', (event) => {
    setDraftCount(event.payload.pending_count)
  })

  const pendingCount = candidates.length + draftCount

  // A new count re-arms the nudge; dismissing only hides the current one.
  useEffect(() => {
    setDismissed(false)
  }, [pendingCount])

  // Fallback refresh (mock/demo + exotic paths): keep the badge in sync
  // after actions taken from surfaces that don't emit the backend event.
  useEffect(() => {
    const handler = () => refetch()
    window.addEventListener('skill-catalog-changed', handler)
    return () => window.removeEventListener('skill-catalog-changed', handler)
  }, [refetch])

  if (location.pathname.startsWith('/extensions/pending')) return null
  if (pendingCount === 0 || dismissed) return null

  return (
    <div className="fixed bottom-4 right-4 z-modal animate-slide-in-from-bottom">
      <div
        role="status"
        className="bg-surface-container-lowest rounded-lg shadow-lg border border-outline-variant p-4 max-w-md"
      >
        <div className="flex items-start gap-3">
          <span className="material-symbols-outlined icon-lg text-primary" aria-hidden="true">lightbulb</span>
          <div className="flex-1">
            <h4 className="font-medium text-on-surface text-sm">
              {intl.formatMessage(
              { id: 'skillProposals.toast.title' },
              { count: pendingCount }
            )}
            </h4>
            <p className="text-xs text-on-surface-variant mt-1">
              {t('skillProposals.toast.description')}
            </p>
            <div className="flex gap-2 mt-3">
              <Button
                onClick={() => navigate('/extensions/pending')}
                size="sm"
              >
                {t('skillProposals.toast.viewButton')}
              </Button>
              <Button
                onClick={() => setDismissed(true)}
                variant="ghost"
                size="sm"
                aria-label={t('skillProposals.toast.closeButton')}
              >
                {t('skillProposals.toast.closeButton')}
              </Button>
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}
