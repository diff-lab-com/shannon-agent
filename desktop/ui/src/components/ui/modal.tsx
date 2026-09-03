import * as React from "react"
import { cva, type VariantProps } from "class-variance-authority"
import { useIntl } from "react-intl"
import { Dialog as DialogPrimitive } from "@base-ui/react/dialog"

import { cn } from "@/lib/utils"
import { Icon } from "@/components/ui/icon"

const modalSizes = cva("w-full", {
  variants: {
    size: {
      sm: "max-w-sm",
      md: "max-w-md",
      lg: "max-w-lg",
      xl: "max-w-xl",
      "2xl": "max-w-2xl",
      full: "max-w-[calc(100vw-2rem)]",
    },
  },
  defaultVariants: {
    size: "md",
  },
})

export interface ModalProps {
  open: boolean
  onClose: () => void
  title?: string
  description?: string
  size?: VariantProps<typeof modalSizes>["size"]
  role?: "dialog" | "alertdialog"
  closeOnBackdrop?: boolean
  closeOnEscape?: boolean
  showCloseButton?: boolean
  closeLabel?: string
  busy?: boolean
  className?: string
  children?: React.ReactNode
}

export function Modal({
  open,
  onClose,
  title,
  description,
  size = "md",
  role = "dialog",
  closeOnBackdrop = true,
  closeOnEscape = true,
  showCloseButton = true,
  closeLabel,
  busy = false,
  className,
  children,
}: ModalProps) {
  const intl = useIntl()
  const closeAriaLabel = closeLabel ?? intl.formatMessage({ id: 'ui.modal.close.aria' })

  const handleOpenChange = React.useCallback(
    (next: boolean, details: DialogPrimitive.Root.ChangeEventDetails) => {
      // Base UI's controlled Dialog only routes the close direction through
      // onOpenChange when the user requests it; we treat every close as a
      // request and gate with the same contract the old Modal had.
      if (next) return
      if (busy) return
      if (details.reason === "outside-press" && !closeOnBackdrop) return
      if (details.reason === "escape-key" && !closeOnEscape) return
      onClose()
    },
    [busy, closeOnBackdrop, closeOnEscape, onClose],
  )

  const hasHeader = Boolean(title || description)

  return (
    <DialogPrimitive.Root open={open} onOpenChange={handleOpenChange}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Backdrop
          className="fixed inset-0 isolate z-flash bg-black/40 backdrop-blur-sm duration-100 data-open:animate-in data-open:fade-in-0 data-closed:animate-out data-closed:fade-out-0"
        />
        <DialogPrimitive.Popup
          role={role}
          aria-modal="true"
          aria-label={title}
          className={cn(
            "fixed top-1/2 left-1/2 z-flash -translate-x-1/2 -translate-y-1/2 w-full max-w-[calc(100%-2rem)] bg-surface-container-lowest rounded-2xl shadow-[var(--shadow-e5)] border border-outline-variant/30 outline-none p-md duration-100 data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95 data-closed:animate-out data-closed:fade-out-0 data-closed:zoom-out-95",
            modalSizes({ size }),
            className
          )}
        >
          {hasHeader && (
            <div className="flex items-start gap-md p-xl pb-md">
              <div className="flex-1 min-w-0">
                {title && (
                  <h2 className="font-headline-md text-on-surface font-bold">{title}</h2>
                )}
                {description && (
                  <p className="text-body-sm text-on-surface-variant mt-xs">{description}</p>
                )}
              </div>
              {showCloseButton && (
                <button
                  type="button"
                  aria-label={closeAriaLabel}
                  disabled={busy}
                  onClick={onClose}
                  className="shrink-0 p-xs rounded-lg text-on-surface-variant hover:bg-surface-container hover:text-on-surface transition-colors disabled:opacity-50 disabled:pointer-events-none"
                >
                  <Icon name="close" />
                </button>
              )}
            </div>
          )}
          {children}
        </DialogPrimitive.Popup>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  )
}

export function ModalBody({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) {
  return (
    <div
      data-slot="modal-body"
      className={cn("px-xl py-md", className)}
      {...props}
    />
  )
}

export function ModalFooter({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) {
  return (
    <div
      data-slot="modal-footer"
      className={cn("flex items-center justify-end gap-sm p-xl pt-md", className)}
      {...props}
    />
  )
}

export { modalSizes }
