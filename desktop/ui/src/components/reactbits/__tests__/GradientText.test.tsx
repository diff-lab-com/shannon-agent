import { describe, expect, it, vi } from 'vitest'
import { render } from '@testing-library/react'
import { GradientText } from '@/components/reactbits/GradientText'

function stubMatchMedia(matches: boolean) {
  Object.defineProperty(window, 'matchMedia', {
    writable: true,
    value: vi.fn().mockImplementation((query: string) => ({
      matches,
      media: query,
      onchange: null,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })),
  })
}

describe('GradientText', () => {
  it('renders text in an animated gradient span when motion is allowed', () => {
    stubMatchMedia(false)
    const { container } = render(<GradientText text="Hello" />)
    const span = container.querySelector('[data-testid="gradient-text"]')
    expect(span).not.toBeNull()
    expect(span?.textContent).toBe('Hello')
    expect(span?.getAttribute('data-reduced')).toBeNull()
  })

  it('falls back to text-primary when reduced-motion is on', () => {
    stubMatchMedia(true)
    const { container } = render(<GradientText text="Hello" />)
    const span = container.querySelector('[data-testid="gradient-text"]')
    expect(span).not.toBeNull()
    expect(span?.getAttribute('data-reduced')).toBe('true')
    expect(span?.className).toMatch(/text-primary/)
  })

  it('applies a custom className', () => {
    stubMatchMedia(false)
    const { container } = render(<GradientText text="Hi" className="font-headline-lg" />)
    const span = container.querySelector('[data-testid="gradient-text"]')
    expect(span?.className).toMatch(/font-headline-lg/)
  })
})