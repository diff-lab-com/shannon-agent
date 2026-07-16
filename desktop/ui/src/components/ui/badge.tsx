import * as React from "react"
import { cva, type VariantProps } from "class-variance-authority"

import { cn } from "@/lib/utils"

const badgeVariants = cva(
  "inline-flex items-center gap-1 rounded-full px-2 py-[2px] font-label-xs font-bold uppercase tracking-wider transition-colors",
  {
    variants: {
      variant: {
        neutral: "bg-surface-container text-on-surface-variant",
        primary: "bg-primary/10 text-primary",
        secondary: "bg-secondary/10 text-secondary",
        tertiary: "bg-tertiary/10 text-tertiary",
        success: "bg-tertiary/10 text-tertiary",
        warning: "bg-tertiary/15 text-on-tertiary-container",
        error: "bg-error/10 text-error",
        outline: "border border-outline-variant/40 text-on-surface-variant",
      },
      size: {
        sm: "px-1.5 py-[1px] text-[10px]",
        md: "",
        lg: "px-2.5 py-[3px] text-label-sm",
      },
    },
    defaultVariants: {
      variant: "neutral",
      size: "md",
    },
  }
)

function Badge({
  className,
  variant,
  size,
  ...props
}: React.HTMLAttributes<HTMLSpanElement> & VariantProps<typeof badgeVariants>) {
  return (
    <span
      data-slot="badge"
      className={cn(badgeVariants({ variant, size }), className)}
      {...props}
    />
  )
}

export { Badge, badgeVariants }
