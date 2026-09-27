/**
 * Review §5 (editor/terminal/shared): FileRefChip's and LinkContextMenu's
 * hand-drawn `role="menu"` surfaces had no menu keyboard semantics — the
 * items were unreachable by keyboard and Escape leaked past the menu into
 * global handlers (T5). Both menus share this small behavior instead of
 * growing a Base UI Menu dependency:
 *   - focus moves to the first item when the menu opens;
 *   - ArrowDown/ArrowUp cycle focus, Home/End jump to first/last;
 *   - Escape closes the menu and stops propagation so the global Escape
 *     handler (e.g. cancel-query) doesn't fire from the same keypress.
 */
import type { KeyboardEvent as ReactKeyboardEvent } from 'react'

const MENU_ITEM_SELECTOR = '[role="menuitem"]'

function menuItems(container: Element | null): HTMLElement[] {
  if (!container) return []
  return Array.from(container.querySelectorAll<HTMLElement>(MENU_ITEM_SELECTOR))
}

function focusAt(items: HTMLElement[], index: number): void {
  const item = items[index]
  item?.focus()
}

/**
 * Focus the first menu item shortly after the menu mounts. Menus render
 * conditionally, so callers run this from an effect keyed on menu state.
 */
export function focusFirstMenuItem(container: Element | null): void {
  // rAF: the menu DOM must be painted/focused after React commits it.
  requestAnimationFrame(() => focusAt(menuItems(container), 0))
}

/** Keydown handler for the `role="menu"` container. */
export function handleMenuKeyDown(
  e: ReactKeyboardEvent,
  container: Element | null,
  onClose: () => void,
): void {
  const items = menuItems(container)
  if (items.length === 0) return
  const currentIndex = items.findIndex(item => item === container?.ownerDocument.activeElement)
  switch (e.key) {
    case 'ArrowDown':
      e.preventDefault()
      e.stopPropagation()
      focusAt(items, currentIndex < 0 ? 0 : (currentIndex + 1) % items.length)
      break
    case 'ArrowUp':
      e.preventDefault()
      e.stopPropagation()
      focusAt(items, currentIndex < 0 ? items.length - 1 : (currentIndex - 1 + items.length) % items.length)
      break
    case 'Home':
      e.preventDefault()
      e.stopPropagation()
      focusAt(items, 0)
      break
    case 'End':
      e.preventDefault()
      e.stopPropagation()
      focusAt(items, items.length - 1)
      break
    case 'Escape':
      // T5: overlay Escape must not also cancel a global behavior.
      e.preventDefault()
      e.stopPropagation()
      onClose()
      break
    default:
      break
  }
}
