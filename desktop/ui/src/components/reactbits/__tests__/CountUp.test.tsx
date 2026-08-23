import { describe, expect, it, vi } from 'vitest'
import { render } from '@testing-library/react'
import { CountUp } from '@/components/reactbits/CountUp'

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

describe('CountUp', () => {
  it('renders the final value immediately when reduced-motion is on', () => {
    stubMatchMedia(true)
    const { container } = render(<CountUp to={42} />)
    const node = container.querySelector('[data-testid="count-up"]')
    expect(node?.textContent).toBe('42')
    expect(node?.getAttribute('data-reduced')).toBe('true')
  })

  it('formats decimals', () => {
    stubMatchMedia(true)
    const { container } = render(<CountUp to={3.14} decimals={2} />)
    expect(container.querySelector('[data-testid="count-up"]')?.textContent).toBe('3.14')
  })

  it('uses tabular-nums class', () => {
    stubMatchMedia(true)
    const { container } = render(<CountUp to={10} />)
    expect(container.querySelector('[data-testid="count-up"]')?.className).toMatch(/tabular-nums/)
  })
})