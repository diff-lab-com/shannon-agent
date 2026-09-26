import { Button } from '@/components/ui/button'
import { Modal, ModalBody, ModalFooter } from '@/components/ui/modal'

interface DeleteSessionModalProps {
  t: (id: string, values?: Record<string, string>) => string
  /** `null` hides the dialog. `permanent` selects the archived-session
   *  永久删除 variant (starker copy, same destructive actions). */
  target: { id: string; title: string; permanent?: boolean } | null
  /** While the delete call is in flight both actions are disabled; a
   *  failure keeps the dialog open (the error surfaces through the shared
   *  banner path) and only a successful delete closes it. */
  pending: boolean
  onCancel: () => void
  onConfirm: () => void
}

// B4 P2-6: the confirm used to be anonymous ("Delete this session?") with
// no pending state — a failed delete silently collapsed into the page-level
// error banner. It now names the target and stays open until the backend
// confirms the row is gone.
export default function DeleteSessionModal({ t, target, pending, onCancel, onConfirm }: DeleteSessionModalProps) {
  const permanent = target?.permanent === true
  const open = target !== null
  return (
    <Modal
      open={open}
      onClose={pending ? () => {} : onCancel}
      role="alertdialog"
      size="sm"
      showCloseButton={false}
    >
      <ModalBody>
        <div className="flex items-center gap-sm mb-md">
          <span className="material-symbols-outlined text-error text-[24px]">delete</span>
          <h3 className="font-headline-md text-on-surface">
            {permanent ? t('chat.delete.permanent.title') : t('chat.delete.title')}
          </h3>
        </div>
        <p className="text-body-md text-on-surface-variant mb-lg">
          {permanent
            ? t('chat.delete.permanent.confirm', { title: target?.title ?? '' })
            : t('chat.delete.confirm', { title: target?.title ?? '' })}
        </p>
      </ModalBody>
      <ModalFooter>
        <Button
          className="px-lg py-sm rounded-xl text-on-surface-variant hover:bg-surface-container"
          disabled={pending}
          onClick={onCancel}
        >
          {t('chat.delete.cancel')}
        </Button>
        <Button
          className="px-lg py-sm rounded-xl bg-error text-on-error hover:bg-error/90 disabled:opacity-60"
          disabled={pending}
          aria-busy={pending || undefined}
          data-testid="delete-session-confirm"
          onClick={onConfirm}
        >
          {pending ? t('chat.delete.working') : t('chat.delete.confirmButton')}
        </Button>
      </ModalFooter>
    </Modal>
  )
}
