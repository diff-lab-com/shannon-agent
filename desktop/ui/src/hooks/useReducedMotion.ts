import { useEffect, useState } from 'react'

const QUERY = '(prefers-reduced-motion: reduce)'

/**
 * Subscribe to the user's `prefers-reduced-motion` setting.
 *
 * Returns `true` when the OS has reduced-motion enabled (or when running in a
 * jsdom test that has stubbed `matchMedia` to opt-in). Components can branch
 * on this to skip rAF loops, expensive transitions, or ReactBits animations.
 *
 * The MediaQueryList listener is attached to `window.matchMedia(QUERY)` and
 * torn down on unmount or query change.
 */
export function useReducedMotion(): boolean {
  const [reduced, setReduced] = useState<boolean>(() => {
    if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') {
      return false
    }
    return window.matchMedia(QUERY).matches
  })

  useEffect(() => {
    if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') {
      return
    }
    const mql = window.matchMedia(QUERY)
    const handler = (event: MediaQueryListEvent) => setReduced(event.matches)
    // Sync once in case the value changed between render and effect.
    setReduced(mql.matches)
    mql.addEventListener('change', handler)
    return () => mql.removeEventListener('change', handler)
  }, [])

  return reduced
}