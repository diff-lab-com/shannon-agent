import * as React from "react"
import { cva, type VariantProps } from "class-variance-authority"

import { cn } from "@/lib/utils"

const cardVariants = cva(
  "rounded-2xl flex flex-col",
  {
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
      interactive: {
        true: "transition-all duration-[var(--duration-normal)] hover:shadow-[var(--shadow-e2)] hover:border-primary/40 focus-visible:outline-2 focus-visible:outline-primary focus-visible:outline-offset-2",
        false: "",
      },
      padding: {
        none: "",
        sm: "p-sm",
        md: "p-md",
        lg: "p-lg",
        xl: "p-xl",
      },
    },
    defaultVariants: {
      variant: "elevated",
      interactive: false,
      padding: "none",
    },
  }
)

function Card({
  className,
  variant,
  interactive,
  padding,
  ...props
}: React.HTMLAttributes<HTMLDivElement> & VariantProps<typeof cardVariants>) {
  return (
    <div
      data-slot="card"
      className={cn(cardVariants({ variant, interactive, padding }), className)}
      {...props}
    />
  )
}

function CardHeader({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) {
  return (
    <div
      data-slot="card-header"
      className={cn("flex flex-col gap-1.5 p-xl pb-md", className)}
      {...props}
    />
  )
}

function CardTitle({ className, ...props }: React.HTMLAttributes<HTMLHeadingElement>) {
  return (
    <h3
      data-slot="card-title"
      className={cn("font-headline-sm text-on-surface font-bold", className)}
      {...props}
    />
  )
}

function CardDescription({ className, ...props }: React.HTMLAttributes<HTMLParagraphElement>) {
  return (
    <p
      data-slot="card-description"
      className={cn("text-body-sm text-on-surface-variant", className)}
      {...props}
    />
  )
}

function CardContent({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) {
  return (
    <div
      data-slot="card-content"
      className={cn("px-xl pb-md", className)}
      {...props}
    />
  )
}

function CardFooter({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) {
  return (
    <div
      data-slot="card-footer"
      className={cn("flex items-center gap-sm px-xl pb-xl", className)}
      {...props}
    />
  )
}

export { Card, CardHeader, CardTitle, CardDescription, CardContent, CardFooter, cardVariants }
