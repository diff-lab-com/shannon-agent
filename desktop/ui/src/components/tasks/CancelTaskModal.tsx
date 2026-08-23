// Confirmation modal for cancelling a running task.
//
// T1.2 — migrated onto the shared Modal primitive (Dialog role) so focus
// management, Esc-to-close, body scroll lock, and backdrop click are
// handled by the primitive instead of hand-rolled hooks/listeners.

import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import { Modal, ModalBody, ModalFooter } from '@/components/ui/modal'

interface CancelTaskModalProps {
  open: boolean
  onCancel: () => void
  onConfirm: () => void
}

export default function CancelTaskModal({ open, onCancel, onConfirm }: CancelTaskModalProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  return (
    <Modal open={open} onClose={onCancel} size="sm">
      <ModalBody>
        <div className="flex items-center gap-sm mb-lg">
          <span className="material-symbols-outlined text-error text-[24px]" aria-hidden="true">warning</span>
          <h3 className="font-headline-md text-on-surface">{t('tasks.cancelTaskModal.title')}</h3>
        </div>
        <p className="text-body-sm text-on-surface-variant">{t('tasks.cancelTaskModal.confirmation')}</p>
      </ModalBody>
      <ModalFooter>
        <Button
          variant="ghost"
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
      </ModalFooter>
    </Modal>
  )
}