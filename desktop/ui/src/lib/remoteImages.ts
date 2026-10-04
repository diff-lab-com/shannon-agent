// remoteImages — P2-4 / R9-④ render-layer gate for remote images in model
// output (docs/plans/2026-10-05-chat-r3-improvement-plan.md §三 P2-4, §五 R9-④).
//
// `![](https://…?d=<context>)` fires a third-party request the moment the
// webview renders it — a tracking/exfiltration channel the model controls.
// Ruling: block by default + per-image confirm + a global allow switch that
// defaults OFF. The gate lives in the renderer (Markdown's LocalImage), NOT
// in the sanitize schema, so links keep their open-in-browser path (P0-A).
//
// Persistence is frontend-local (localStorage, like the theme/density keys)
// on purpose: the Rust config contract is untouched, and the preference is
// per-device UI behavior rather than engine configuration.

const ALLOW_KEY = 'shannon.chat.allowRemoteImages'

/** The Tauri asset protocol's Linux shape is an http:// URL — see the
 *  `http://asset.localhost` sanitize note in Markdown's tests. It serves
 *  local files, so it must never count as remote. */
const ASSET_HOSTS = new Set(['asset.localhost'])

let allowAll: boolean = readStoredAllow()

function readStoredAllow(): boolean {
  try {
    return localStorage.getItem(ALLOW_KEY) === '1'
  } catch {
    return false
  }
}

const listeners = new Set<() => void>()

export function isRemoteImagesAllowed(): boolean {
  return allowAll
}

/** Global switch (AdvancedSettings). Persists synchronously — a failed write
 *  only means the choice doesn't survive a restart, so there is no error
 *  path worth surfacing. */
export function setRemoteImagesAllowed(next: boolean): void {
  if (next === allowAll) return
  allowAll = next
  try {
    localStorage.setItem(ALLOW_KEY, next ? '1' : '0')
  } catch { /* noop */ }
  for (const notify of listeners) notify()
}

export function subscribeRemoteImagesAllowed(notify: () => void): () => void {
  listeners.add(notify)
  return () => { listeners.delete(notify) }
}

function remoteHostname(url: string): string | null {
  try {
    const u = new URL(url)
    if (ASSET_HOSTS.has(u.hostname)) return null
    return u.hostname
  } catch {
    return null
  }
}

/**
 * Should this src be held behind the per-image confirm? Only requests that
 * leave the machine qualify: absolute http(s) URLs and scheme-relative ones
 * (`//host/x.png` — resolves under the page protocol). Everything else is
 * self-contained or app-local: `file://`, `/abs/path`, `asset://`, `data:`,
 * `blob:`, and page-relative paths never contact a third party.
 */
export function isGatedRemoteImageSrc(src: string): boolean {
  const s = src.trim()
  if (/^https?:\/\//i.test(s)) return remoteHostname(s) !== null
  if (s.startsWith('//')) return remoteHostname(`https:${s}`) !== null
  return false
}

/** Display host for the gate placeholder; falls back to the raw src when the
 *  URL doesn't parse (only reachable for exotic scheme-relative forms). */
export function remoteImageHost(src: string): string {
  return remoteHostname(/^https?:\/\//i.test(src.trim()) ? src.trim() : `https:${src.trim()}`) ?? src
}
