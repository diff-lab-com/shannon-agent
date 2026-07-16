// useModalFocus — focus trap + restoration for modal dialogs.
//
// Usage:
//   const containerRef = useRef<HTMLDivElement>(null)
//   useModalFocus(open, containerRef)
//   return open ? <div ref={containerRef} role="dialog">...</div> : null
//
// Behaviour:
//   - On open: save the currently focused element, then move focus to the
//     first focusable child of the container (or the container itself if
//     none is focusable). The container should carry role="dialog".
//   - While open: trap Tab / Shift+Tab so focus cycles within the container.
//   - On close (including unmount while open): restore focus to the element
//     that was focused before the modal opened.
//
// The hook is dependency-free and works with React 19. It intentionally
// does NOT handle Edge cases like portals rendering into a separate root —
// pass a ref to whatever DOM node acts as the visual modal container.

import { useEffect } from 'react'

const FOCUSABLE_SELECTOR = [
  'a[href]',
  'button:not([disabled])',
  'textarea:not([disabled])',
  'input:not([disabled]):not([type="hidden"])',
  'select:not([disabled])',
  '[tabindex]:not([tabindex="-1"])',
]
  .map((s) => `${s}:not([inert])`)
  .join(',')

function getFocusable(container: HTMLElement): HTMLElement[] {
  return Array.from(
    container.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR),
  ).filter(
    (el) =>
      el.offsetWidth > 0 ||
      el.offsetHeight > 0 ||
      el.getClientRects().length > 0,
  )
}

export function useModalFocus(
  open: boolean,
  containerRef: React.RefObject<HTMLElement | null>,
): void {
  useEffect(() => {
    if (!open) return

    // Save the element that had focus before the modal opened so we can
    // restore it on close. Read it synchronously — the modal's own
    // focus-shift hasn't happened yet at this point in the commit.
    const previouslyFocused = document.activeElement as HTMLElement | null

    const container = containerRef.current
    if (container) {
      // Move focus into the modal.
      const focusable = getFocusable(container)
      const target = focusable[0] ?? container
      // container itself may not be focusable; give it a temporary
      // tabIndex only if no focusable child exists.
      if (focusable.length === 0 && container.tabIndex < 0) {
        container.tabIndex = -1
      }
      target.focus()
    }

    function handleKeyDown(e: KeyboardEvent) {
      if (e.key !== 'Tab') return
      const root = containerRef.current
      if (!root) return
      const focusable = getFocusable(root)
      if (focusable.length === 0) {
        e.preventDefault()
        return
      }
      const first = focusable[0]
      const last = focusable[focusable.length - 1]
      const active = document.activeElement as HTMLElement | null

      if (e.shiftKey) {
        // Shift+Tab: if focus is on (or before) the first element, wrap to last.
        if (active === first || !root.contains(active)) {
          e.preventDefault()
          last.focus()
        }
      } else {
        // Tab: if focus is on (or after) the last element, wrap to first.
        if (active === last || !root.contains(active)) {
          e.preventDefault()
          first.focus()
        }
      }
    }

    document.addEventListener('keydown', handleKeyDown)

    return () => {
      document.removeEventListener('keydown', handleKeyDown)
      // Restore focus to whatever had it before the modal opened.
      // Guard for the element having been removed from the DOM.
      if (previouslyFocused && document.body.contains(previouslyFocused)) {
        previouslyFocused.focus()
      }
    }
  }, [open, containerRef])
}
