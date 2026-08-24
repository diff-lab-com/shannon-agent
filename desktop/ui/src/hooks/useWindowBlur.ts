import { useEffect, useState } from 'react'

/**
 * Returns `true` while the desktop window is unfocused.
 *
 * Listens for the standard `visibilitychange` event on `document` (handles
 * browser demo mode + most Tauri releases) and the `tauri://focus` /
 * `tauri://blur` events emitted by the Tauri webview (handles the actual
 * desktop case where `document.hidden` lags behind the OS focus state).
 *
 * Animation components can branch on this to stop their rAF loops when the
 * window is backgrounded — keeps CPU flat while the user is in another app.
 */
export function useWindowBlur(): boolean {
  const [blurred, setBlurred] = useState<boolean>(() => {
    if (typeof document === 'undefined') return false
    return document.hidden
  })

  useEffect(() => {
    if (typeof document === 'undefined') return
    const onVisibility = () => setBlurred(document.hidden)
    document.addEventListener('visibilitychange', onVisibility)
    setBlurred(document.hidden)
    return () => document.removeEventListener('visibilitychange', onVisibility)
  }, [])

  // Tauri window events — best-effort. They may not be available in the
  // browser demo build; the dynamic import keeps the hook usable there too.
  useEffect(() => {
    if (typeof window === 'undefined') return
    let unlistenFocus: (() => void) | undefined
    let unlistenBlur: (() => void) | undefined
    let cancelled = false

    import('@tauri-apps/api/event')
      .then(({ listen }) => {
        if (cancelled) return
        listen('tauri://focus', () => setBlurred(false))
          .then(fn => {
            if (cancelled) fn()
            else unlistenFocus = fn
          })
          .catch(() => {})
        listen('tauri://blur', () => setBlurred(true))
          .then(fn => {
            if (cancelled) fn()
            else unlistenBlur = fn
          })
          .catch(() => {})
      })
      .catch(() => {})

    return () => {
      cancelled = true
      unlistenFocus?.()
      unlistenBlur?.()
    }
  }, [])

  return blurred
}