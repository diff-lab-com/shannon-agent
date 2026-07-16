import * as React from "react"
import { cva, type VariantProps } from "class-variance-authority"
import { X } from "lucide-react"
import { useIntl } from "react-intl"

import { cn } from "@/lib/utils"
import { useModalFocus } from "@/hooks/useModalFocus"

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
  busy = false,
  className,
  children,
}: ModalProps) {
  const containerRef = React.useRef<HTMLDivElement>(null)
  const intl = useIntl()
  useModalFocus(open, containerRef)

  React.useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && closeOnEscape && !busy) onClose()
    }
    document.addEventListener("keydown", onKey)
    return () => document.removeEventListener("keydown", onKey)
  }, [open, closeOnEscape, busy, onClose])

  React.useEffect(() => {
    if (!open) return
    const prev = document.body.style.overflow
    document.body.style.overflow = "hidden"
    return () => {
      document.body.style.overflow = prev
    }
  }, [open])

  if (!open) return null

  const hasHeader = Boolean(title || description)

  return (
    <div
      className="fixed inset-0 z-[100] flex items-center justify-center bg-black/40 backdrop-blur-sm p-md"
      onClick={(e) => {
        if (closeOnBackdrop && !busy && e.target === e.currentTarget) onClose()
      }}
    >
      <div
        ref={containerRef}
        role={role}
        aria-modal="true"
        aria-label={title}
        className={cn(
          "relative bg-surface-container-lowest rounded-2xl shadow-[var(--shadow-e5)] border border-outline-variant/30",
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
                aria-label={intl.formatMessage({ id: 'ui.modal.close.aria' })}
                disabled={busy}
                onClick={onClose}
                className="shrink-0 p-xs rounded-lg text-on-surface-variant hover:bg-surface-container hover:text-on-surface transition-colors disabled:opacity-50 disabled:pointer-events-none"
              >
                <X className="size-4" />
              </button>
            )}
          </div>
        )}
        {children}
      </div>
    </div>
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
