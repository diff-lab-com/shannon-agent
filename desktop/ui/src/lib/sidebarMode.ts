// sidebarMode — D4 (research 2026-10-05-d4-interface-mode §1.6): the simple
// sidebar mode doubles as the "concise" render branch. Non-developer readers
// get the reduced header (ToolGroupCard hides the technical tool-name
// summary); dev mode — and expansion, the escape hatch — show everything.
// No new setting: this reuses the Sidebar mode key, whose toggle is the
// single writer.
//
// Chat components must not import Sidebar (it pulls the React tree — same
// reason density.ts inline-reads the key), so the read lives here and the
// reactive hook subscribes to the event Sidebar dispatches after each write
// (plus 'storage' for cross-window updates).

import { useSyncExternalStore } from 'react'

export type SidebarModeValue = 'simple' | 'advanced'

const SIDEBAR_MODE_KEY = 'shannon-sidebar-mode'
/** Dispatched by Sidebar's mode toggle after it persists the new value. */
const MODE_CHANGED_EVENT = 'shannon-sidebar-mode-changed'

/** Raw stored value ('dev' | 'basic') → the semantic mode; unset, unknown
 *  or unavailable storage defaults to 'simple' (mirrors useSidebarMode). */
export function readSidebarMode(): SidebarModeValue {
  if (typeof window === 'undefined') return 'simple'
  try {
    const raw = window.localStorage.getItem(SIDEBAR_MODE_KEY)
    if (raw === 'dev') return 'advanced'
    if (raw === 'basic') return 'simple'
  } catch { /* noop */ }
  return 'simple'
}

function subscribe(onStoreChange: () => void): () => void {
  window.addEventListener(MODE_CHANGED_EVENT, onStoreChange)
  window.addEventListener('storage', onStoreChange)
  return () => {
    window.removeEventListener(MODE_CHANGED_EVENT, onStoreChange)
    window.removeEventListener('storage', onStoreChange)
  }
}

/** Reactive read for non-Sidebar consumers. The snapshot is the stable
 *  'simple' | 'advanced' primitive, so re-renders only fire on real flips. */
export function useSidebarModeValue(): SidebarModeValue {
  return useSyncExternalStore(subscribe, readSidebarMode)
}
