import { useEffect, useRef } from 'react'
import { Button } from '@/components/ui/button'
import { useModalFocus } from '@/hooks/useModalFocus'

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
  const containerRef = useRef<HTMLDivElement>(null)
  useModalFocus(open, containerRef)

  useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape' && !busy) onCancel() }
    document.addEventListener('keydown', onKey)
    return () => document.removeEventListener('keydown', onKey)
  }, [open, busy, onCancel])

  if (!open) return null

  return (
    <div
      className="fixed inset-0 z-[100] bg-black/40 backdrop-blur-sm flex items-center justify-center p-md"
      onClick={() => { if (!busy) onCancel() }}
    >
      <div
        ref={containerRef}
        role="alertdialog"
        aria-modal="true"
        aria-label={title}
        className="bg-surface-container-lowest rounded-2xl p-xl shadow-2xl border border-outline-variant/30 max-w-sm w-full"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center gap-sm mb-md">
          <span className={`material-symbols-outlined text-[24px] ${destructive ? 'text-error' : 'text-primary'}`}>
            {destructive ? 'warning' : 'help'}
          </span>
          <h3 className="font-headline-md text-on-surface">{title}</h3>
        </div>
        <p className="text-body-md text-on-surface-variant mb-lg">{message}</p>
        <div className="flex justify-end gap-sm">
          <Button
            variant="ghost"
            disabled={busy}
            className="px-lg py-sm rounded-xl text-on-surface-variant hover:bg-surface-container cursor-pointer"
            onClick={onCancel}
          >
            {cancelLabel}
          </Button>
          <Button
            disabled={busy}
            className={`px-lg py-sm rounded-xl text-on-primary cursor-pointer disabled:opacity-50 ${
              destructive ? 'bg-error hover:bg-error/90' : 'bg-primary hover:bg-primary/90'
            }`}
            onClick={onConfirm}
          >
            {busy ? '…' : confirmLabel}
          </Button>
        </div>
      </div>
    </div>
  )
}
