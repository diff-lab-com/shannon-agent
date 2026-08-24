// SidePanel — right-side drawer that owns the same a11y contract as Modal:
// role="dialog" + aria-modal="true", focus trap, Escape to close,
// scroll-lock the body, click backdrop to dismiss (configurable).
//
// T1.2 — added so the legacy `fixed inset-0 z-50 flex justify-end`
// pattern in TaskDetailDrawer/RoutineDetailDrawer/etc. can be replaced
// without losing focus/Esc/scroll-lock semantics. Distinct from Modal
// because the panel is anchored to the right edge, full height, no
// rounded corners, and the backdrop is a separate sibling element so
// the panel can overflow past the backdrop if needed.
//
// T1.2/R1a — Shannon's `ui/drawer.tsx` shim (deleted in R1a) is now
// gone; SidePanel is the canonical right-edge drawer primitive.

import * as React from 'react'
import { useIntl } from 'react-intl'
import { useModalFocus } from '@/hooks/useModalFocus'
import { cn } from '@/lib/utils'

export interface SidePanelProps {
  open: boolean
  onClose: () => void
  title?: string
  width?: string
  closeOnBackdrop?: boolean
  closeOnEscape?: boolean
  ariaLabel?: string
  className?: string
  children?: React.ReactNode
}

export function SidePanel({
  open,
  onClose,
  title,
  width = '400px',
  closeOnBackdrop = true,
  closeOnEscape = true,
  ariaLabel,
  className,
  children,
}: SidePanelProps) {
  const containerRef = React.useRef<HTMLDivElement>(null)
  useModalFocus(open, containerRef)

  React.useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && closeOnEscape) onClose()
    }
    document.addEventListener('keydown', onKey)
    return () => document.removeEventListener('keydown', onKey)
  }, [open, closeOnEscape, onClose])

  React.useEffect(() => {
    if (!open) return
    const prev = document.body.style.overflow
    document.body.style.overflow = 'hidden'
    return () => {
      document.body.style.overflow = prev
    }
  }, [open])

  if (!open) return null

  return (
    <div
      className="fixed inset-0 z-50 flex justify-end"
      onClick={(e) => {
        if (closeOnBackdrop && e.target === e.currentTarget) onClose()
      }}
    >
      <div
        className="absolute inset-0 bg-black/40 backdrop-blur-sm"
        onClick={() => closeOnBackdrop && onClose()}
      />
      <div
        ref={containerRef}
        role="dialog"
        aria-modal="true"
        aria-label={ariaLabel ?? title}
        className={cn(
          'relative h-full w-full bg-surface-container-lowest shadow-2xl border-l border-outline-variant/30 overflow-y-auto',
          className,
        )}
        style={{ maxWidth: width }}
      >
        {children}
      </div>
    </div>
  )
}

export function SidePanelHeader({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) {
  return (
    <div
      data-slot="side-panel-header"
      className={cn('flex items-center justify-between px-xl py-md border-b border-outline-variant/30', className)}
      {...props}
    />
  )
}

export function SidePanelTitle({ children }: { children?: React.ReactNode }) {
  return <h2 className="font-headline-md text-on-surface font-bold">{children}</h2>
}

export function SidePanelBody({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) {
  return <div data-slot="side-panel-body" className={cn('p-xl', className)} {...props} />
}

export function SidePanelCloseButton({ onClick, label }: { onClick: () => void; label?: string }) {
  const intl = useIntl()
  const ariaLabel = label ?? intl.formatMessage({ id: 'ui.modal.close.aria' })
  return (
    <button
      type="button"
      aria-label={ariaLabel}
      onClick={onClick}
      className="p-sm rounded-lg hover:bg-surface-container text-on-surface-variant cursor-pointer focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
    >
      <span className="material-symbols-outlined" aria-hidden="true">close</span>
    </button>
  )
}