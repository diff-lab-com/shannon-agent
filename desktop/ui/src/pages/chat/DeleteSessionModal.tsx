import { Button } from '@/components/ui/button'
import { Modal, ModalBody, ModalFooter } from '@/components/ui/modal'

interface DeleteSessionModalProps {
  t: (id: string) => string
  deleteTarget: string | null
  onCancel: () => void
  onConfirm: () => void
}

export default function DeleteSessionModal({ t, deleteTarget, onCancel, onConfirm }: DeleteSessionModalProps) {
  return (
    <Modal
      open={deleteTarget !== null}
      onClose={onCancel}
      role="alertdialog"
      size="sm"
      showCloseButton={false}
    >
      <ModalBody>
        <div className="flex items-center gap-sm mb-md">
          <span className="material-symbols-outlined text-error text-[24px]">delete</span>
          <h3 className="font-headline-md text-on-surface">{t('chat.delete.title')}</h3>
        </div>
        <p className="text-body-md text-on-surface-variant mb-lg">{t('chat.delete.confirm')}</p>
      </ModalBody>
      <ModalFooter>
        <Button className="px-lg py-sm rounded-xl text-on-surface-variant hover:bg-surface-container" onClick={onCancel}>{t('chat.delete.cancel')}</Button>
        <Button className="px-lg py-sm rounded-xl bg-error text-on-error hover:bg-error/90" onClick={onConfirm}>{t('chat.delete.confirmButton')}</Button>
      </ModalFooter>
    </Modal>
  )
}