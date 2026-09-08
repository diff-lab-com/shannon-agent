import { cn } from '@/lib/utils'
import { useReducedMotion } from '@/hooks/useReducedMotion'

interface GradientTextProps {
  text: string
  /** Tailwind classes forwarded onto the inner span. Use this for size/weight. */
  className?: string
  /**
   * Tailwind gradient utility names without the `bg-gradient-to-r` prefix.
   * Defaults to a primary-anchored shimmer so the brand wordmark keeps the
   * theme's primary hue in all 12 themes (the old via-tertiary/to-secondary
   * defaults mixed in amber stops — material tertiary is #855000 — and the
   * small wordmark read brown).
   */
  fromClass?: string
  viaClass?: string
  toClass?: string
  /**
   * Background-clip to text behavior: clip the gradient through transparent
   * text. Defaults to true.
   */
  clip?: boolean
}

/**
 * CSS-only ReactBits Gradient Text equivalent. Renders the text with a
 * background-clip gradient. Honors `prefers-reduced-motion` by falling back
 * to a single-tone `text-primary` color instead of the animated gradient.
 *
 * No WebGL, no OGL, no rAF loop. When reduced-motion is off, the gradient
 * pans slowly via the `animate-gradient-pan` utility defined in `index.css`.
 */
export function GradientText({
  text,
  className,
  fromClass = 'from-primary',
  viaClass = 'via-primary-container',
  toClass = 'to-primary',
  clip = true,
}: GradientTextProps) {
  const reduced = useReducedMotion()

  if (reduced) {
    return (
      <span
        className={cn('text-primary', className)}
        data-testid="gradient-text"
        data-reduced="true"
      >
        {text}
      </span>
    )
  }

  return (
    <span
      data-testid="gradient-text"
      className={cn(
        'inline-block bg-gradient-to-r bg-[length:200%_auto] animate-gradient-pan',
        fromClass,
        viaClass,
        toClass,
        clip ? 'bg-clip-text text-transparent' : 'text-foreground',
        className,
      )}
    >
      {text}
    </span>
  )
}