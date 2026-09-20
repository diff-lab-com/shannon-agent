// Toast notification for pending skill proposals.
//
// Fixed position bottom-right. Listens to skill-proposal-available events
// (the primary channel — backend emits this when a new candidate arrives
// or when one is approved/rejected from Advanced Settings). The
// skill-catalog-changed listener is the fallback path used when another
// surface saves a candidate without going through the toast flow.

import { useEffect, useState } from 'react'
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import { useTauriEventValidated } from '@/hooks/useTauriEventValidated'
import { skillLoop } from '@/lib/tauri-api'
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

  // 2026-09 P0-5: keep the badge in sync after actions taken from the
  // Advanced Settings "Save as skill" modal — the backend may not emit
  // `skill-proposal-available` for that path. Cheap re-fetch (mock only);
  // real backend already maintains the source of truth.
  useEffect(() => {
    const handler = () => {
      skillLoop.listProposals().then((list) => {
        const next = list.length
        setPendingCount(next)
        setVisible(next > 0)
      }).catch(() => { /* best-effort */ })
    }
    window.addEventListener('skill-catalog-changed', handler)
    return () => window.removeEventListener('skill-catalog-changed', handler)
  }, [])

  if (!visible || pendingCount === 0) return null

  const handleView = () => {
    onOpenReview()
    setVisible(false)
  }

  const handleDismiss = () => {
    setVisible(false)
  }

  return (
    <div className="fixed bottom-4 right-4 z-modal animate-slide-in-from-bottom">
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
