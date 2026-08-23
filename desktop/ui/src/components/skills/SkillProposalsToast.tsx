// Toast notification for pending skill proposals.
//
// Fixed position bottom-right. Listens to skill-proposal-available events
// and shows count with "View" button that opens the review panel.

import { useState } from 'react'
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import { useTauriEventValidated } from '@/hooks/useTauriEventValidated'
import type { SkillProposalCountPayload } from '@/types'

interface SkillProposalsToastProps {
  onOpenReview: () => void
}

export default function SkillProposalsToast({ onOpenReview }: SkillProposalsToastProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const [pendingCount, setPendingCount] = useState(0)
  const [visible, setVisible] = useState(false)

  useTauriEventValidated<SkillProposalCountPayload>('skill-proposal-available', (event) => {
    setPendingCount(event.payload.pending_count)
    if (event.payload.pending_count > 0) {
      setVisible(true)
    }
  })

  if (!visible || pendingCount === 0) return null

  const handleView = () => {
    onOpenReview()
    setVisible(false)
  }

  const handleDismiss = () => {
    setVisible(false)
  }

  return (
    <div className="fixed bottom-4 right-4 z-50 animate-slide-in-from-bottom">
      <div className="bg-surface-container-lowest rounded-lg shadow-lg border border-outline-variant p-4 max-w-md">
        <div className="flex items-start gap-3">
          <span className="material-symbols-outlined icon-lg text-primary">lightbulb</span>
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
                onClick={handleView}
                size="sm"
              >
                {t('skillProposals.toast.viewButton')}
              </Button>
              <Button
                onClick={handleDismiss}
                variant="ghost"
                size="sm"
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
