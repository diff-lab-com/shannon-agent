import type { ReactNode } from 'react'
import { useReducedMotion } from '@/hooks/useReducedMotion'
import { useWindowBlur } from '@/hooks/useWindowBlur'

interface MotionGuardProps {
  /**
   * If `true`, animations render normally. If `false`, the `children` are
   * rendered as-is (no animation wrapper, no rAF cost).
   */
  active: boolean
  children: ReactNode
  /**
   * Optional static fallback. Defaults to `children` — but consumers can
   * pass a stripped-down variant when reduced-motion is on (e.g. a plain
   * text node instead of a Gradient Text effect).
   */
  fallback?: ReactNode
}

/**
 * Renders `fallback` (or `children` if not provided) when the user has
 * reduced-motion enabled or the window is unfocused, otherwise renders
 * `children`. Lets animation components opt out of expensive rAF loops and
 * still show their content in a static form.
 */
export function MotionGuard({ active, children, fallback }: MotionGuardProps) {
  const reduced = useReducedMotion()
  const blurred = useWindowBlur()
  if (!active || reduced || blurred) {
    return <>{fallback ?? children}</>
  }
  return <>{children}</>
}