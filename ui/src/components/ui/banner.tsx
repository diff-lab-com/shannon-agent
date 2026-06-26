import * as React from "react"
import { cva, type VariantProps } from "class-variance-authority"

import { cn } from "@/lib/utils"

const bannerVariants = cva(
  "shannon-banner flex gap-md",
  {
    variants: {
      variant: {
        // Full-width bar pinned to the top of a surface (e.g. "API key missing").
        bar: "items-start px-lg py-sm border-b shrink-0",
        // Self-contained card rendered inline in the page flow (e.g. form errors).
        card: "items-center px-md py-sm rounded-xl border",
      },
      tone: {
        info: "bg-secondary-container/40 border-secondary/30",
        warning: "bg-tertiary-container/40 border-tertiary/30",
        error: "bg-error-container/40 border-error/30",
        success: "bg-primary-container/40 border-primary/30",
      },
    },
    defaultVariants: {
      variant: "bar",
      tone: "info",
    },
  }
)

export interface BannerProps
  extends React.HTMLAttributes<HTMLDivElement>,
    VariantProps<typeof bannerVariants> {
  /** When provided, renders a dismiss (close) button at the trailing edge. */
  onDismiss?: () => void
  /** Accessible label for the dismiss button (pass a translated string). */
  dismissLabel?: string
}

/**
 * Dismissible status banner. Renders role="alert" for the error tone and
 * role="status" otherwise; both carry aria-live="polite".
 *
 * Call sites compose the icon + message (+ optional CTA) as children — the
 * primitive supplies the surface, tone color, layout, and an optional dismiss
 * button so the two hand-rolled banners (Chat API-key, Tasks error) share one
 * accessible, token-consistent implementation.
 */
function Banner({
  className,
  variant,
  tone,
  onDismiss,
  dismissLabel,
  role,
  children,
  ...props
}: BannerProps) {
  const resolvedRole = role ?? (tone === "error" ? "alert" : "status")
  return (
    <div
      data-slot="banner"
      role={resolvedRole}
      aria-live="polite"
      className={cn(bannerVariants({ variant, tone }), className)}
      {...props}
    >
      {children}
      {onDismiss && (
        <button
          type="button"
          onClick={onDismiss}
          aria-label={dismissLabel}
          className="shrink-0 p-xs rounded text-on-surface-variant hover:text-on-surface hover:bg-surface-container/60 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary cursor-pointer"
        >
          <span className="material-symbols-outlined icon-sm" aria-hidden="true">
            close
          </span>
        </button>
      )}
    </div>
  )
}

export { Banner, bannerVariants }
