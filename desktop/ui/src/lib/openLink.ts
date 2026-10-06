// openLink — the single entry point for opening external http(s) links
// (docs/plans/2026-09-25-desktop-chat-ui-open-and-artifact-design.md §4 P0-A).
//
// Decision §5-6 (拍板): the default destination is the right dock's web tab
// (`panel`); Alt+click / the link context menu override per click, and the
// global setting (`shannon.link.target`) decides the plain-click behavior.
// Before the web tab host registers a router (P1-E not mounted — demo mode,
// non-chat pages), `panel` degrades to the system browser so the click is
// always honored.

import { toast } from 'sonner'
import { messageFor } from '@/i18n'
import { openExternal } from '@/lib/tauri-api'
import { errorMessage } from '@/lib/errorToast'

export type LinkTarget = 'panel' | 'browser'

const LINK_TARGET_KEY = 'shannon.link.target'

/** Plain-click destination. Defaults to `panel` (decision §5-6). */
export function getLinkTarget(): LinkTarget {
  try {
    return localStorage.getItem(LINK_TARGET_KEY) === 'browser' ? 'browser' : 'panel'
  } catch {
    return 'panel'
  }
}

export function setLinkTarget(target: LinkTarget): void {
  try {
    localStorage.setItem(LINK_TARGET_KEY, target)
  } catch {
    /* storage unavailable — keep runtime value only */
  }
}

export function isExternalHttpUrl(href: string): boolean {
  return /^https?:\/\//i.test(href)
}

/** Registered by the dock's web-tab host (T6/P1-E); null degrades to browser. */
type PanelRouteFn = (url: string) => void
let panelRouter: PanelRouteFn | null = null

export function registerLinkPanelRouter(route: PanelRouteFn | null): void {
  panelRouter = route
}

/** Test hook — resets the module-level router between tests. */
export function resetLinkPanelRouterForTests(): void {
  panelRouter = null
}

/**
 * Open an external link honoring the target-resolution order of §4 P0-A ⑦:
 * per-call override > global setting. `panel` without a registered web-tab
 * router falls back to the system browser.
 */
export async function openLink(href: string, override?: LinkTarget): Promise<void> {
  const target = override ?? getLinkTarget()
  if (target === 'panel' && panelRouter) {
    panelRouter(href)
    return
  }
  try {
    await openExternal(href)
  } catch (e) {
    toast.error(messageFor('link.open.failed'), { description: errorMessage(e) })
  }
}
