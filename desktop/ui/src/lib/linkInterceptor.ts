// linkInterceptor — document-level capture-phase click/auxclick handling
// (docs/plans/2026-09-25-desktop-chat-ui-open-and-artifact-design.md §4 P0-A).
//
// Why this exists: every bare `<a href="https://…" target="_blank">` in the
// app (chat markdown, document renderer, extensions pages, report modal)
// used to rely on WebView defaults, which in Tauri usually means "nothing
// happens". One capture-phase interceptor fixes them all without touching
// each call site:
//
//   * external http(s) → openLink (panel by default, Alt/middle → browser)
//   * `#anchor` and app-relative `/route` → untouched (footnotes, router)
//   * anything else (relative `foo.md`, unknown schemes) → blocked, so the
//     webview never navigates away from the app (§3 P0-2).

import { isExternalHttpUrl, openLink } from '@/lib/openLink'

export type HrefKind = 'external-http' | 'anchor' | 'app' | 'other'

/** Classify an anchor href into the interceptor's action buckets. */
export function classifyHref(href: string): HrefKind {
  if (href.startsWith('#')) return 'anchor'
  if (isExternalHttpUrl(href)) return 'external-http'
  if (href.startsWith('/')) return 'app'
  return 'other'
}

let installed = false
let clickHandler: ((e: MouseEvent) => void) | null = null

/** Idempotent — StrictMode double-mounts are harmless. */
export function installLinkInterception(): void {
  if (installed || typeof document === 'undefined') return
  installed = true

  const handle = (e: MouseEvent) => {
    // Primary click + middle click only; contextmenu is handled separately
    // by the LinkContextMenuHost.
    if (e.button !== 0 && e.button !== 1) return
    const anchor = (e.target as Element | null)?.closest?.('a[href]') as HTMLAnchorElement | null
    if (!anchor) return
    const href = anchor.getAttribute('href') ?? ''
    const kind = classifyHref(href)
    if (kind === 'external-http') {
      e.preventDefault()
      // Alt+click: temporarily flip to the browser (§4 P0-A ⑦); middle
      // click keeps the classic "open in background app" reading.
      void openLink(href, e.button === 1 || e.altKey ? 'browser' : undefined)
    } else if (kind === 'other') {
      e.preventDefault()
    }
  }

  clickHandler = handle
  document.addEventListener('click', handle, true)
  document.addEventListener('auxclick', handle, true)
}

/** Test hook — uninstall (removing the listeners) and allow a fresh install. */
export function resetLinkInterceptionForTests(): void {
  if (clickHandler && typeof document !== 'undefined') {
    document.removeEventListener('click', clickHandler, true)
    document.removeEventListener('auxclick', clickHandler, true)
  }
  clickHandler = null
  installed = false
}
