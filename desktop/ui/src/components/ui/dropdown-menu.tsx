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
// contract. If the composition ever migrates, regenerate the shadcn
// primitive (`pnpm dlx shadcn@latest add dropdown-menu`) and move call sites
// to the Base UI composition — focus behavior then moves to
// <Menu.Item>'s roving tabindex out of the box. (The previously parked
// dropdown-menu.prim.tsx was 0-ref inventory and was removed in the
// 2026-08-26 audit cleanup.)
//
// For now: legacy focus hook + Base UI surface tokens. Compat shim.
// (B4 audit: this is NOT dead code — it backs the composer's "+" menu, the
// session rail's ⋯ menu, and the sidebar's split-New menu. The unused
// `triggerRef` prop that once hinted at the Base UI migration was removed.)

export interface DropdownMenuItem {
  id: string
  label: string
  icon?: string
  disabled?: boolean
  destructive?: boolean
  /** Stable e2e/aria anchor (e.g. the composer's memory-bypass item keeps
   *  the retired standalone button's `memory-bypass-toggle`). */
  testid?: string
  onSelect?: () => void
}

/** R2 audit W6 (02c 设计稿 S1) — a non-interactive divider row, the
 *  data-driven counterpart of shadcn's DropdownMenuSeparator for this
 *  items[]-array API. Create via the `DropdownMenuSeparator(id)` helper. */
export interface DropdownMenuSeparatorItem {
  kind: 'separator'
  id: string
}

/** Anything the menu can render: a regular item or a separator. Plain
 *  `DropdownMenuItem[]` call sites remain assignable (existing menus never
 *  grow separators unless they opt in). */
export type DropdownMenuEntry = DropdownMenuItem | DropdownMenuSeparatorItem

/** shadcn-style `DropdownMenuSeparator` factory for the flat items[] API. */
export function DropdownMenuSeparator(id: string): DropdownMenuSeparatorItem {
  return { kind: 'separator', id }
}

const isSeparator = (item: DropdownMenuEntry): item is DropdownMenuSeparatorItem =>
  'kind' in item

/** Positive predicate — TS does not narrow a union through a negated
 *  custom predicate, so the focus-roving guards use this form to reach
 *  `.disabled`/`.onSelect` on the item branch. */
const isRegularItem = (item: DropdownMenuEntry): item is DropdownMenuItem =>
  !isSeparator(item)

export interface DropdownMenuProps {
  open: boolean
  onClose: () => void
  items: DropdownMenuEntry[]
  align?: "start" | "end"
  className?: string
  ariaLabel?: string
}

/**
 * Shannon DropdownMenu primitive. Preserved legacy focus-roving +
 * outside-click + Escape semantics; surface tokens aligned with the
 * shadcn base-nova primitives so a future call site can migrate to the
 * Base UI composition directly.
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
    // Separators are never focusable — the roving index only ever points at
    // real items (guards below repeat the skip).
    const firstEnabled = items.findIndex((i) => isRegularItem(i) && !i.disabled)
    setFocusIndex(firstEnabled)
  }, [open, items])

  React.useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent) => {
      // T5 (review P1-6): an open menu OWNS its keys — every handled branch
      // stops propagation so the document-level handler here can't let the
      // same keydown also reach window-level global shortcuts (previously
      // Escape closed the menu AND cancelled a running query).
      if (e.key === "Escape") {
        e.preventDefault()
        e.stopPropagation()
        onClose()
        return
      }
      if (e.key === "ArrowDown") {
        e.preventDefault()
        e.stopPropagation()
        setFocusIndex((cur) => {
          for (let i = cur + 1; i < items.length; i++) {
            const it = items[i]
            if (isRegularItem(it) && !it.disabled) return i
          }
          return cur
        })
      } else if (e.key === "ArrowUp") {
        e.preventDefault()
        e.stopPropagation()
        setFocusIndex((cur) => {
          for (let i = cur - 1; i >= 0; i--) {
            const it = items[i]
            if (isRegularItem(it) && !it.disabled) return i
          }
          return cur
        })
      } else if (e.key === "Enter" || e.key === " ") {
        e.preventDefault()
        e.stopPropagation()
        const item = items[focusIndex]
        if (item && isRegularItem(item) && !item.disabled) {
          item.onSelect?.()
          onClose()
        }
      } else if (e.key === "Tab") {
        e.preventDefault()
        e.stopPropagation()
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
        // G3 (UI review 2026-09-29): menu content carries the shared
        // glass-overlay floating material (bg/blur/border/shadow from the
        // utility) + the panel-in entrance; no per-component surface recipe.
        "glass-overlay animate-panel-in absolute z-modal min-w-[200px] rounded-xl py-xs",
        align === "end" ? "right-0 top-full mt-sm" : "left-0 top-full mt-sm",
        className
      )}
    >
      {items.map((item, index) =>
        isSeparator(item) ? (
          // shadcn DropdownMenuSeparator convention: a plain non-interactive
          // row, excluded from the roving index (no data-menu-item-index).
          <div
            key={item.id}
            role="separator"
            aria-orientation="horizontal"
            data-slot="dropdown-menu-separator"
            className="my-xs h-px bg-outline-variant/25"
          />
        ) : (
          <button
            key={item.id}
            type="button"
            role="menuitem"
            data-menu-item-index={index}
            data-testid={item.testid}
            aria-disabled={item.disabled}
            disabled={item.disabled}
            className={cn(
              "w-full text-left px-md py-sm flex items-center gap-sm text-label-md transition-colors",
              "focus:bg-primary/10 focus:text-primary focus:outline-none",
              item.disabled
                ? "opacity-40 cursor-not-allowed"
                : "text-on-surface hover:bg-primary/10 hover:text-primary cursor-pointer",
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
              <span className="material-symbols-outlined icon-md shrink-0" aria-hidden="true">
                {item.icon}
              </span>
            )}
            <span className="truncate">{item.label}</span>
          </button>
        )
      )}
    </div>
  )
}