import { useEffect, useState } from 'react'
import { cn } from '@/lib/utils'
import { useReducedMotion } from '@/hooks/useReducedMotion'
import { useWindowBlur } from '@/hooks/useWindowBlur'

interface TextLoopProps {
  /** Items to cycle through, in order. */
  items: string[]
  /** Per-item display time in ms. Default 2500. */
  intervalMs?: number
  /** Tailwind classes forwarded to the visible span. */
  className?: string
  /** Optional aria-label override (defaults to the currently visible item). */
  ariaLabel?: string
  /** Announce item changes to screen readers (aria-live="polite")?
   * Default false — a polite live region firing every intervalMs is
   * chatty for assistive-tech users; enable only when the rotation
   * itself is the content. */
  live?: boolean
}

/**
 * CSS-only ReactBits Text Loop equivalent. Cycles through `items` using
 * setInterval (paused when `prefers-reduced-motion` is on or the window is
 * unfocused — honors T2.1 guards).
 *
 * No CSS animation by default — the swap is instant so it works inside
 * a single line of body copy without layout jitter. A `transition-opacity`
 * class can be added by the caller if a fade is desired.
 */
export function TextLoop({ items, intervalMs = 2500, className, ariaLabel, live = false }: TextLoopProps) {
  const reduced = useReducedMotion()
  const blurred = useWindowBlur()
  const [index, setIndex] = useState(0)

  useEffect(() => {
    if (reduced || blurred || items.length <= 1) return
    const id = setInterval(() => {
      setIndex(i => (i + 1) % items.length)
    }, intervalMs)
    return () => clearInterval(id)
  }, [items.length, intervalMs, reduced, blurred])

  if (items.length === 0) return null

  return (
    <span
      aria-label={ariaLabel ?? items[index]}
      aria-live={live && !reduced ? 'polite' : 'off'}
      className={cn('inline-block align-baseline', className)}
      data-testid="text-loop"
      data-reduced={reduced ? 'true' : 'false'}
    >
      {items[index]}
    </span>
  )
}