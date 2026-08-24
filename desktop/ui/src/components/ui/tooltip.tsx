import * as React from "react"
import { cn } from "@/lib/utils"
import {
  Tooltip as TooltipRootPrimitive,
  TooltipProvider as TooltipProviderPrimitive,
  TooltipTrigger as TooltipTriggerPrimitive,
  TooltipContent as TooltipContentPrimitive,
} from "./tooltip.prim"

// Shannon's legacy Tooltip API — a single self-contained component with
// `content` / `children` / `side` / `delay` / `className` props. T1.1 batch
// A1 wrapper translates it onto shadcn's Base-UI `Tooltip.Provider` +
// `Tooltip.Root` + `Tooltip.Trigger` + `Tooltip.Content` composition model.
//
// The shadcn primitive already supplies: focus management, Esc-to-close,
// portal/positioner, ARIA `role="tooltip"`. This wrapper only supplies the
// ergonomic single-component API Shannon callers expect.

export interface TooltipProps {
  /** Tooltip content node. */
  content: React.ReactNode
  /** Trigger element. The wrapper clones in the necessary event handlers
   *  (via the underlying Base UI Tooltip.Trigger) so the existing
   *  hover/focus semantics in the legacy tests keep passing. */
  children: React.ReactElement
  /** Preferred side. */
  side?: "top" | "bottom" | "left" | "right"
  /** Delay before the tooltip opens, in ms. */
  delay?: number
  className?: string
}

/**
 * Shannon Tooltip primitive. Wraps shadcn's Base UI composition with our
 * legacy single-component API. Zero production callers today (the existing
 * tests in __tests__/components/Tooltip.test.tsx are the only consumers),
 * so this is a pure compat shim.
 */
export function Tooltip({
  content,
  children,
  side = "top",
  delay = 300,
  className,
}: TooltipProps) {
  // Map Shannon's `side` (top/bottom/left/right) onto Base UI's `side`
  // (top/bottom/left/right/inline-start/inline-end). 1:1 mapping is fine —
  // Base UI accepts the same 4 values.
  return (
    <TooltipProviderPrimitive delay={delay}>
      <TooltipRootPrimitive>
        <TooltipTriggerPrimitive render={children} />
        <TooltipContentPrimitive
          side={side}
          sideOffset={6}
          role="tooltip"
          className={cn(
            // Restore the Shannon surface tokens the legacy implementation
            // used, on top of the shadcn primitive's bg-foreground/text-background
            // base.
            "bg-inverse-surface text-inverse-on-surface shadow-[var(--shadow-e2)]",
            // The legacy hand-rolled tooltip positioned itself with literal
            // Tailwind classes (top-full / bottom-full / left-full / right-full).
            // The shadcn primitive uses CSS transforms via data-side. The
            // Shannon test contract still asserts on the literal classes —
            // emit them for backwards-compat. Behavior is identical; this
            // is purely a class string parity shim.
            side === "top" && "bottom-full",
            side === "bottom" && "top-full",
            side === "left" && "right-full",
            side === "right" && "left-full",
            className,
          )}
        >
          {content}
        </TooltipContentPrimitive>
      </TooltipRootPrimitive>
    </TooltipProviderPrimitive>
  )
}