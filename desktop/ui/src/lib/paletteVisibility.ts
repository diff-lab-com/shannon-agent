// paletteVisibility — the shannon:* window-event bridge for CommandPalette
// open/close (Aurora redesign 2026-10, F-9 companion rule).
//
// Layout owns the palette state; the surfaces that must react to it (the
// chat page's RightDock, which drops its backdrop-filter via `.dock-solid`
// while a floating layer is up so the on-screen glass budget ≤ 4 holds)
// live outside Layout's render tree. The event follows the established
// `shannon:toggle-help` / `shannon:toggle-palette` window-event convention.

export const PALETTE_VISIBILITY_EVENT = 'shannon:palette-visibility'

export interface PaletteVisibilityDetail {
  open: boolean
}

/** Layout-side announcement — fires on mount (initial state) and on change. */
export function dispatchPaletteVisibility(open: boolean): void {
  window.dispatchEvent(
    new CustomEvent<PaletteVisibilityDetail>(PALETTE_VISIBILITY_EVENT, { detail: { open } }),
  )
}

/** Subscriber-side helper — returns the unlisten function. */
export function onPaletteVisibility(handler: (open: boolean) => void): () => void {
  const listener = (e: Event) => {
    const detail = (e as CustomEvent<PaletteVisibilityDetail>).detail
    handler(Boolean(detail?.open))
  }
  window.addEventListener(PALETTE_VISIBILITY_EVENT, listener)
  return () => window.removeEventListener(PALETTE_VISIBILITY_EVENT, listener)
}
