// Confirmation modal for cancelling a running task.
//
// MD3 tokens. Click outside or "Keep Running" dismisses without action.

import { useRef } from 'react'
import { Button } from '@/components/ui/button'
import { useIntl } from 'react-intl'
import { useModalFocus } from '@/hooks/useModalFocus'

interface CancelTaskModalProps {
  open: boolean
  onCancel: () => void
  onConfirm: () => void
}

export default function CancelTaskModal({ open, onCancel, onConfirm }: CancelTaskModalProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const containerRef = useRef<HTMLDivElement>(null)
  useModalFocus(open, containerRef)
  if (!open) return null
  return (
    <div
      className="fixed inset-0 z-[200] flex items-center justify-center bg-black/30 backdrop-blur-sm"
      onClick={onCancel}
    >
      <div
        ref={containerRef}
        className="bg-surface-container-lowest rounded-2xl border border-outline-variant/20 shadow-2xl p-xl max-w-sm w-full mx-md"
        role="dialog"
        aria-modal="true"
        onClick={e => e.stopPropagation()}
      >
        <div className="flex items-center gap-sm mb-lg">
          <span className="material-symbols-outlined text-error text-[24px]">warning</span>
          <h3 className="font-headline-md text-on-surface">{t('tasks.cancelTaskModal.title')}</h3>
        </div>
        <p className="text-body-sm text-on-surface-variant mb-lg">{t('tasks.cancelTaskModal.confirmation')}</p>
        <div className="flex gap-sm justify-end">
          <Button
            className="px-md py-sm rounded-lg border border-outline-variant text-on-surface-variant font-label-md cursor-pointer"
            onClick={onCancel}
          >
            {t('tasks.cancelTaskModal.keepRunning')}
          </Button>
          <Button
            className="px-md py-sm rounded-lg bg-error text-on-error font-label-md cursor-pointer hover:brightness-110"
            onClick={onConfirm}
          >
            {t('tasks.cancelTaskModal.cancelTask')}
          </Button>
        </div>
      </div>
    </div>
  )
}
