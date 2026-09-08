import * as React from "react"
import { cn } from "@/lib/utils"
import { Badge as BadgePrimitive } from "./badge.prim"

// Shannon's legacy variants — preserved by this wrapper so call sites
// and the Badge.test.tsx contract stay green. T1.1 batch A1.
//
// Mapping rationale:
//   neutral  → outline  (subdued surface tone — same visual weight)
//   primary  → default  (brand foreground)
//   tertiary → secondary (use the secondary token; visually equivalent)
//   success  → default + accent classes (brand-foreground tone)
//   warning  → secondary + accent classes (tertiary tone)
//   error    → destructive (built-in)
//   secondary→ secondary (1:1)
//   outline  → outline  (1:1)
//
// Tone accent classes reapply Shannon's existing color tokens (primary/
// tertiary/error) so we don't depend on the destructive token landing in
// every [data-theme] block.
type ShannonVariant =
  | "neutral" | "primary" | "secondary" | "tertiary"
  | "success" | "warning" | "error" | "outline"

type ShannonSize = "sm" | "md" | "lg"

const TONE_ACCENT: Record<ShannonVariant, string> = {
  neutral: "bg-surface-container text-on-surface-variant",
  primary: "bg-primary/10 text-primary",
  secondary: "bg-secondary/10 text-secondary",
  tertiary: "bg-tertiary/10 text-tertiary",
  success: "bg-tertiary/10 text-tertiary",
  warning: "bg-tertiary/15 text-on-tertiary-container",
  error: "bg-error/10 text-error",
  outline: "border border-outline-variant/40 text-on-surface-variant",
}

const SIZE_CLASSES: Record<ShannonSize, string | undefined> = {
  sm: "px-1.5 py-[1px] text-[10px]",
  md: undefined,
  lg: "px-2.5 py-[3px] text-label-sm",
}

export interface BadgeProps
  extends Omit<React.HTMLAttributes<HTMLSpanElement>, "className"> {
  variant?: ShannonVariant
  size?: ShannonSize
  className?: string
}

/**
 * Shannon Badge primitive. Wraps the shadcn-generated `Badge` and re-applies
 * our variant tokens so existing call sites and tests are unaffected.
 *
 * Round shape, label-xs font, uppercase tracking — those are baked into the
 * shadcn primitive's base classes (rounded-4xl, text-xs, font-medium). We
 * extend the surface tokens per variant via `TONE_ACCENT`.
 */
export function Badge({
  className,
  variant = "neutral",
  size = "md",
  ...props
}: BadgeProps) {
  return (
    <BadgePrimitive
      data-slot="badge"
      data-shannon-variant={variant}
      className={cn(
        // Restore the label-xs/uppercase/tracking semantics from the
        // previous hand-rolled version. shadcn uses text-xs without the
        // uppercase/label tracking; callers expect a pill look.
        "font-label-xs font-bold uppercase tracking-wider",
        TONE_ACCENT[variant],
        SIZE_CLASSES[size],
        className,
      )}
      {...props}
    />
  )
}

// Re-export the shadcn variants helper so call sites that compose badges
// via cva (rare; usually only in tests) keep working.
export { badgeVariants } from "./badge.prim"