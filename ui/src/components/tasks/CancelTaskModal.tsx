// Confirmation modal for cancelling a running task.
//
// MD3 tokens. Click outside or "Keep Running" dismisses without action.

import { Button } from '@/components/ui/button'

interface CancelTaskModalProps {
  open: boolean
  onCancel: () => void
  onConfirm: () => void
}

export default function CancelTaskModal({ open, onCancel, onConfirm }: CancelTaskModalProps) {
  if (!open) return null
  return (
    <div
      className="fixed inset-0 z-[200] flex items-center justify-center bg-black/30 backdrop-blur-sm"
      onClick={onCancel}
    >
      <div
        className="bg-surface-container-lowest rounded-2xl border border-outline-variant/20 shadow-2xl p-xl max-w-sm w-full mx-md"
        role="dialog"
        aria-modal="true"
        onClick={e => e.stopPropagation()}
      >
        <div className="flex items-center gap-sm mb-lg">
          <span className="material-symbols-outlined text-error text-[24px]">warning</span>
          <h3 className="font-headline-md text-on-surface">Cancel Task?</h3>
        </div>
        <p className="text-body-sm text-on-surface-variant mb-lg">This will stop the running task. Any progress will be lost.</p>
        <div className="flex gap-sm justify-end">
          <Button
            className="px-md py-sm rounded-lg border border-outline-variant text-on-surface-variant font-label-md cursor-pointer"
            onClick={onCancel}
          >
            Keep Running
          </Button>
          <Button
            className="px-md py-sm rounded-lg bg-error text-on-error font-label-md cursor-pointer hover:brightness-110"
            onClick={onConfirm}
          >
            Cancel Task
          </Button>
        </div>
      </div>
    </div>
  )
}
