import { Modal, ModalBody, ModalFooter } from '@/components/ui/modal'
import { Button } from '@/components/ui/button'

export interface ConfirmDialogProps {
  open: boolean
  title: string
  message: string
  confirmLabel: string
  cancelLabel: string
  destructive?: boolean
  busy?: boolean
  onConfirm: () => void
  onCancel: () => void
}

export function ConfirmDialog({
  open,
  title,
  message,
  confirmLabel,
  cancelLabel,
  destructive = false,
  busy = false,
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  return (
    <Modal
      open={open}
      onClose={onCancel}
      title={title}
      size="sm"
      role="alertdialog"
      busy={busy}
      showCloseButton={!destructive}
    >
      <ModalBody className="pt-0">
        <div className="flex items-start gap-sm">
          <span
            className={`material-symbols-outlined text-[24px] mt-[2px] ${
              destructive ? 'text-error' : 'text-primary'
            }`}
            aria-hidden="true"
          >
            {destructive ? 'warning' : 'help'}
          </span>
          <p className="text-body-md text-on-surface-variant">{message}</p>
        </div>
      </ModalBody>
      <ModalFooter className="pt-0">
        <Button
          variant="ghost"
          disabled={busy}
          className="px-md py-sm rounded-xl text-on-surface-variant hover:bg-surface-container cursor-pointer"
          onClick={onCancel}
        >
          {cancelLabel}
        </Button>
        <Button
          disabled={busy}
          className={`px-md py-sm rounded-xl text-on-primary cursor-pointer disabled:opacity-50 ${
            destructive ? 'bg-error hover:bg-error/90' : 'bg-primary hover:bg-primary/90'
          }`}
          onClick={onConfirm}
        >
          {busy ? '…' : confirmLabel}
        </Button>
      </ModalFooter>
    </Modal>
  )
}
