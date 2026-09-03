import * as React from "react"

import { cn } from "@/lib/utils"

// Shannon's legacy DropdownMenu — controlled `open` + `onClose` + flat
// `items[]` array. The legacy hand-rolled implementation handled focus
// roving via local state (focusIndex) plus a document-level keydown
// listener; the test contract (DropdownMenu.test.tsx) is built on top of
// that semantics.
//
// T1.1 batch A2 — we adopt the shadcn base-nova primitive's *surface tokens
// (rounded-xl + bg-surface-container-lowest/95 + shadow-[var(--shadow-e3)])
// but preserve Shannon's local focus-management because Base UI's
// <Menu.Item> roving tabindex requires a working anchor trigger that the
// legacy API does not expose. Without a real trigger, Base UI never
// initializes focus on open and ArrowDown does nothing — breaking the test
// contract. When a real call site lands and `triggerRef` is wired to a
// real <button>, regenerate the shadcn primitive
// (`pnpm dlx shadcn@latest add dropdown-menu`) and swap this wrapper for
// the Base UI composition — focus behavior then moves to <Menu.Item>'s
// roving tabindex out of the box. (The previously parked
// dropdown-menu.prim.tsx was 0-ref inventory and was removed in the
// 2026-08-26 audit cleanup.)
//
// For now: legacy focus hook + Base UI surface tokens. Compat shim.

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

/**
 * Shannon DropdownMenu primitive. Preserved legacy focus-roving +
 * outside-click + Escape semantics; surface tokens aligned with the
 * shadcn base-nova primitives so a future call site can migrate to the
 * Base UI composition directly.
 *
 * Zero production callers today (only __tests__/components/DropdownMenu.test.tsx),
 * so this is a compat shim. Once a real call site lands, prefer the
 * base-nova primitives (DropdownMenuTrigger, DropdownMenuContent, …) directly.
 */
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
      data-slot="dropdown-menu-content"
      className={cn(
        // Match the shadcn base-nova surface tokens (rounded-xl, surface
        // container lowest, shadow-e3) so a future migration to the
        // Base UI composition will look identical.
        "absolute z-modal min-w-[200px] bg-surface-container-lowest/95 backdrop-blur-lg rounded-xl border border-outline-variant/20 shadow-[var(--shadow-e3)] py-xs",
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