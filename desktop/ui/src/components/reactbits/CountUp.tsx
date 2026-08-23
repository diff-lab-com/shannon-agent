import { useEffect, useRef, useState } from 'react'
import { cn } from '@/lib/utils'
import { useReducedMotion } from '@/hooks/useReducedMotion'
import { useWindowBlur } from '@/hooks/useWindowBlur'

interface CountUpProps {
  /** Final value to count up to. */
  to: number
  /** Duration of the animation in ms. Default 1200. */
  durationMs?: number
  /** Starting value. Default 0. */
  from?: number
  /** Decimals to show. Default 0. */
  decimals?: number
  /** Optional locale for number formatting. */
  locale?: string
  /** Tailwind classes forwarded to the rendered <span>. */
  className?: string
}

/**
 * CSS-free CountUp equivalent. Uses requestAnimationFrame for a smooth
 * count when reduced-motion is off and the window is focused. Renders the
 * final number immediately when reduced-motion is on.
 *
 * Pauses on window blur (T2.1 guard) — no background rAF.
 */
export function CountUp({
  to,
  durationMs = 1200,
  from = 0,
  decimals = 0,
  locale,
  className,
}: CountUpProps) {
  const reduced = useReducedMotion()
  const blurred = useWindowBlur()
  const [value, setValue] = useState(from)
  const startRef = useRef<number | null>(null)
  const rafRef = useRef<number | null>(null)

  useEffect(() => {
    if (reduced || blurred) {
      setValue(to)
      return
    }
    setValue(from)
    startRef.current = null
    const step = (now: number) => {
      if (startRef.current == null) startRef.current = now
      const elapsed = now - startRef.current
      const t = Math.min(1, elapsed / durationMs)
      // ease-out cubic
      const eased = 1 - Math.pow(1 - t, 3)
      setValue(from + (to - from) * eased)
      if (t < 1) rafRef.current = requestAnimationFrame(step)
    }
    rafRef.current = requestAnimationFrame(step)
    return () => {
      if (rafRef.current != null) cancelAnimationFrame(rafRef.current)
    }
  }, [to, from, durationMs, reduced, blurred])

  const formatted = value.toLocaleString(locale, {
    minimumFractionDigits: decimals,
    maximumFractionDigits: decimals,
  })

  return (
    <span
      className={cn('tabular-nums', className)}
      data-testid="count-up"
      data-reduced={reduced ? 'true' : 'false'}
    >
      {formatted}
    </span>
  )
}