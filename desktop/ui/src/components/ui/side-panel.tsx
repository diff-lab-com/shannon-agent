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
// R1c — re-implemented on top of `@base-ui/react/dialog` primitives
// (the same swap Modal did in R1b). External API is frozen: callers
// (RoutineDetailDrawer, SkillDetailDrawer, TaskDetailDrawer) keep
// their existing `<SidePanel>` / `<SidePanelHeader>` / `<SidePanelTitle>`
// / `<SidePanelBody>` / `<SidePanelCloseButton>` composition untouched.

import * as React from 'react'
import { useIntl } from 'react-intl'
import { Dialog as DialogPrimitive } from '@base-ui/react/dialog'
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
  const handleOpenChange = React.useCallback(
    (next: boolean, details: DialogPrimitive.Root.ChangeEventDetails) => {
      // Same gating contract as R1b's Modal: ignore every close unless
      // the reason matches what the caller opted into.
      if (next) return
      if (details.reason === 'outside-press' && !closeOnBackdrop) return
      if (details.reason === 'escape-key' && !closeOnEscape) return
      onClose()
    },
    [closeOnBackdrop, closeOnEscape, onClose],
  )

  return (
    <DialogPrimitive.Root open={open} onOpenChange={handleOpenChange}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Backdrop
          className={cn(
            'fixed inset-0 isolate z-50 bg-black/40 backdrop-blur-sm duration-100',
            'data-open:animate-in data-open:fade-in-0',
            'data-closed:animate-out data-closed:fade-out-0',
          )}
        />
        <DialogPrimitive.Popup
          role="dialog"
          aria-modal="true"
          aria-label={ariaLabel ?? title}
          className={cn(
            'fixed inset-y-0 right-0 z-50 h-full w-full bg-surface-container-lowest shadow-2xl border-l border-outline-variant/30 overflow-y-auto outline-none',
            'duration-100',
            'data-open:animate-in data-open:fade-in-0 data-open:slide-in-from-right',
            'data-closed:animate-out data-closed:fade-out-0 data-closed:slide-out-to-right',
            className,
          )}
          style={{ maxWidth: width }}
        >
          {children}
        </DialogPrimitive.Popup>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
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
