import * as React from "react"
import { cn } from "@/lib/utils"
import { cva, type VariantProps } from "class-variance-authority"

import {
  Card as CardPrimitive,
  CardHeader as CardHeaderPrimitive,
  CardContent as CardContentPrimitive,
  CardFooter as CardFooterPrimitive,
} from "./card.prim"

// Shannon's legacy Card variant system — preserved by this wrapper so the
// existing ConnectionsSettings caller and Card.test.tsx stay green.
//
// shadcn's `Card` is a single default style with a `size` axis (default|sm).
// Shannon needs:
//   variant:   elevated | outlined | glass | accent (4 distinct surfaces)
//   padding:   none | sm | md | lg | xl            (5 padding tokens)
//   interactive: boolean                          (hover/focus surface lift)
//
// We re-apply the legacy cva on top of the shadcn primitive so the underlying
// div inherits shadcn's base styles (bg-card, ring-1, group/card, etc.) and
// Shannon's tokens override only the dimensions/colors that differ.
const shannonCardVariants = cva("", {
  variants: {
    variant: {
      elevated:
        "bg-surface-container-lowest border border-outline-variant/30 shadow-[var(--shadow-e1)]",
      outlined:
        "bg-surface-container-lowest border border-outline-variant/50",
      glass:
        "glass-card border border-outline-variant/40",
      accent:
        "bg-surface-container-lowest border border-outline-variant/30 shadow-[var(--shadow-e1)]",
    },
    padding: {
      none: "",
      sm: "p-sm",
      md: "p-md",
      lg: "p-lg",
      xl: "p-xl",
    },
    interactive: {
      true: "transition-all duration-[var(--duration-normal)] hover:shadow-[var(--shadow-e2)] hover:border-primary/40 focus-visible:outline-2 focus-visible:outline-primary focus-visible:outline-offset-2",
      false: "",
    },
  },
  defaultVariants: {
    variant: "elevated",
    padding: "none",
    interactive: false,
  },
})

export interface CardProps
  extends Omit<React.HTMLAttributes<HTMLDivElement>, "className">,
    VariantProps<typeof shannonCardVariants> {
  className?: string
}

/**
 * Shannon Card primitive. Wraps the shadcn-generated `Card` with our legacy
 * variant/padding/interactive cva so the existing API is unchanged.
 *
 * `data-slot="card"` is preserved so Card.test.tsx can still query it.
 */
export function Card({
  className,
  variant,
  padding,
  interactive,
  ...props
}: CardProps) {
  return (
    <CardPrimitive
      data-slot="card"
      className={cn(shannonCardVariants({ variant, padding, interactive }), className)}
      {...props}
    />
  )
}

// CardHeader / CardContent / CardFooter: thin wrappers around the shadcn
// primitives. Slot attributes preserved so Card.test.tsx keeps passing.
function CardHeader({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) {
  return <CardHeaderPrimitive data-slot="card-header" className={cn("flex flex-col gap-1.5 p-xl pb-md", className)} {...props} />
}

function CardContent({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) {
  return <CardContentPrimitive data-slot="card-content" className={cn("px-xl pb-md", className)} {...props} />
}

function CardFooter({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) {
  return <CardFooterPrimitive data-slot="card-footer" className={cn("flex items-center gap-sm px-xl pb-xl", className)} {...props} />
}

// CardTitle / CardDescription render plain HTML elements directly rather
// than going through the shadcn primitives. The shadcn versions emit <div>
// tags (via useRender with a default tag of 'div'), but the Shannon contract
// is semantic markup (<h3>/<p>) — both for screen readers and for tests that
// may query by heading/paragraph role. Re-emitting the elements here keeps
// the legacy behavior with no caller churn.
function CardTitle({ className, ...props }: React.HTMLAttributes<HTMLHeadingElement>) {
  return <h3 data-slot="card-title" className={cn("font-headline-sm text-on-surface font-bold", className)} {...props} />
}

function CardDescription({ className, ...props }: React.HTMLAttributes<HTMLParagraphElement>) {
  return <p data-slot="card-description" className={cn("text-body-sm text-on-surface-variant", className)} {...props} />
}

export {
  CardHeader,
  CardTitle,
  CardDescription,
  CardContent,
  CardFooter,
  shannonCardVariants as cardVariants,
}