import * as React from "react"
import { cn } from "@/lib/utils"

export interface DropdownMenuItem {
  id: string
  label: string
  icon?: string
  disabled?: boolean
  destructive?: boolean
  onSelect?: () => void
}

export interface DropdownMenuProps {
  open: boolean
  onClose: () => void
  items: DropdownMenuItem[]
  align?: "start" | "end"
  className?: string
  triggerRef?: React.RefObject<HTMLElement | null>
  ariaLabel?: string
}

export function DropdownMenu({
  open,
  onClose,
  items,
  align = "end",
  className,
  ariaLabel,
}: DropdownMenuProps) {
  const menuRef = React.useRef<HTMLDivElement>(null)
  const [focusIndex, setFocusIndex] = React.useState(-1)

  React.useEffect(() => {
    if (!open) {
      setFocusIndex(-1)
      return
    }
    const firstEnabled = items.findIndex((i) => !i.disabled)
    setFocusIndex(firstEnabled)
  }, [open, items])

  React.useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault()
        onClose()
        return
      }
      if (e.key === "ArrowDown") {
        e.preventDefault()
        setFocusIndex((cur) => {
          for (let i = cur + 1; i < items.length; i++) {
            if (!items[i].disabled) return i
          }
          return cur
        })
      } else if (e.key === "ArrowUp") {
        e.preventDefault()
        setFocusIndex((cur) => {
          for (let i = cur - 1; i >= 0; i--) {
            if (!items[i].disabled) return i
          }
          return cur
        })
      } else if (e.key === "Enter" || e.key === " ") {
        e.preventDefault()
        const item = items[focusIndex]
        if (item && !item.disabled) {
          item.onSelect?.()
          onClose()
        }
      } else if (e.key === "Tab") {
        e.preventDefault()
        onClose()
      }
    }
    document.addEventListener("keydown", onKey)
    return () => document.removeEventListener("keydown", onKey)
  }, [open, items, focusIndex, onClose])

  React.useEffect(() => {
    if (!open) return
    const onPointer = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        onClose()
      }
    }
    document.addEventListener("mousedown", onPointer)
    return () => document.removeEventListener("mousedown", onPointer)
  }, [open, onClose])

  React.useEffect(() => {
    if (!open) return
    const item = menuRef.current?.querySelector<HTMLElement>(
      `[data-menu-item-index="${focusIndex}"]`
    )
    item?.focus()
  }, [focusIndex, open])

  if (!open) return null

  return (
    <div
      ref={menuRef}
      role="menu"
      aria-label={ariaLabel}
      className={cn(
        "absolute z-50 min-w-[200px] bg-surface-container-lowest/95 backdrop-blur-lg rounded-xl border border-outline-variant/20 shadow-[var(--shadow-e3)] py-xs",
        align === "end" ? "right-0 top-full mt-sm" : "left-0 top-full mt-sm",
        className
      )}
    >
      {items.map((item, index) => (
        <button
          key={item.id}
          type="button"
          role="menuitem"
          data-menu-item-index={index}
          aria-disabled={item.disabled}
          disabled={item.disabled}
          className={cn(
            "w-full text-left px-md py-sm flex items-center gap-sm text-label-md transition-colors",
            "focus:bg-primary/10 focus:text-primary focus:outline-none",
            item.disabled
              ? "opacity-40 cursor-not-allowed"
              : "text-on-surface hover:bg-primary/5 hover:text-primary cursor-pointer",
            item.destructive && "text-error hover:bg-error/10 hover:text-error focus:bg-error/10 focus:text-error"
          )}
          onClick={() => {
            if (item.disabled) return
            item.onSelect?.()
            onClose()
          }}
          onMouseEnter={() => !item.disabled && setFocusIndex(index)}
        >
          {item.icon && (
            <span className="material-symbols-outlined text-[18px] shrink-0" aria-hidden="true">
              {item.icon}
            </span>
          )}
          <span className="truncate">{item.label}</span>
        </button>
      ))}
    </div>
  )
}
