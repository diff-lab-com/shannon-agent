/**
 * Platform helpers for keyboard shortcut display.
 *
 * useKeyboardShortcuts accepts both metaKey and ctrlKey, so the *display*
 * of shortcut hints should match the user's OS convention: ⌘ on macOS,
 * Ctrl elsewhere.
 */

function isMac(): boolean {
  if (typeof navigator === 'undefined') return false
  const uaData = (navigator as Navigator & { userAgentData?: { platform?: string } }).userAgentData
  const ua = (uaData?.platform ?? navigator.platform ?? '').toLowerCase()
  return ua.includes('mac')
}

export function modKey(): string {
  return isMac() ? '⌘' : 'Ctrl'
}

export function modSeparator(): string {
  return isMac() ? '' : '+'
}

export function formatShortcut(key: string): string {
  return `${modKey()}${modSeparator()}${key}`
}
