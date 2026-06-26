import * as React from "react"
import { cn } from "@/lib/utils"

export interface TooltipProps {
  content: React.ReactNode
  children: React.ReactElement
  side?: "top" | "bottom" | "left" | "right"
  delay?: number
  className?: string
}

export function Tooltip({
  content,
  children,
  side = "top",
  delay = 300,
  className,
}: TooltipProps) {
  const [open, setOpen] = React.useState(false)
  const timerRef = React.useRef<ReturnType<typeof setTimeout> | null>(null)
  const id = React.useId()

  const show = React.useCallback(() => {
    if (timerRef.current) clearTimeout(timerRef.current)
    timerRef.current = setTimeout(() => setOpen(true), delay)
  }, [delay])

  const hide = React.useCallback(() => {
    if (timerRef.current) clearTimeout(timerRef.current)
    setOpen(false)
  }, [])

  React.useEffect(() => {
    return () => {
      if (timerRef.current) clearTimeout(timerRef.current)
    }
  }, [])

  const sideClasses = {
    top: "bottom-full left-1/2 -translate-x-1/2 mb-xs",
    bottom: "top-full left-1/2 -translate-x-1/2 mt-xs",
    left: "right-full top-1/2 -translate-y-1/2 mr-xs",
    right: "left-full top-1/2 -translate-y-1/2 ml-xs",
  }

  const childProps: React.HTMLAttributes<HTMLElement> = {
    "aria-describedby": open ? id : undefined,
    onMouseEnter: show,
    onMouseLeave: hide,
    onFocus: show,
    onBlur: hide,
  }

  return (
    <span className="relative inline-flex">
      {React.cloneElement(children, childProps)}
      {open && (
        <span
          role="tooltip"
          id={id}
          className={cn(
            "absolute z-[200] pointer-events-none px-sm py-xs rounded-md bg-inverse-surface text-inverse-on-surface text-label-xs font-medium shadow-[var(--shadow-e2)] whitespace-nowrap",
            sideClasses[side],
            className
          )}
        >
          {content}
        </span>
      )}
    </span>
  )
}
