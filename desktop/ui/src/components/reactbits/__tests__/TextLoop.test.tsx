import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest'
import { act, render } from '@testing-library/react'
import { TextLoop } from '@/components/reactbits/TextLoop'

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

describe('TextLoop', () => {
  beforeEach(() => { vi.useFakeTimers() })
  afterEach(() => { vi.useRealTimers() })

  it('renders the first item initially', () => {
    stubMatchMedia(false)
    const { container } = render(<TextLoop items={['A', 'B', 'C']} />)
    expect(container.querySelector('[data-testid="text-loop"]')?.textContent).toBe('A')
  })

  it('rotates through items on interval', () => {
    stubMatchMedia(false)
    const { container } = render(<TextLoop items={['A', 'B', 'C']} intervalMs={1000} />)
    const node = () => container.querySelector('[data-testid="text-loop"]')
    expect(node()?.textContent).toBe('A')
    act(() => { vi.advanceTimersByTime(1000) })
    expect(node()?.textContent).toBe('B')
    act(() => { vi.advanceTimersByTime(1000) })
    expect(node()?.textContent).toBe('C')
    act(() => { vi.advanceTimersByTime(1000) })
    expect(node()?.textContent).toBe('A')
  })

  it('does not rotate when reduced-motion is on', () => {
    stubMatchMedia(true)
    const { container } = render(<TextLoop items={['A', 'B']} intervalMs={1000} />)
    const node = () => container.querySelector('[data-testid="text-loop"]')
    expect(node()?.textContent).toBe('A')
    expect(node()?.getAttribute('data-reduced')).toBe('true')
    act(() => { vi.advanceTimersByTime(5000) })
    expect(node()?.textContent).toBe('A')
  })

  it('returns null for empty items', () => {
    stubMatchMedia(false)
    const { container } = render(<TextLoop items={[]} />)
    expect(container.querySelector('[data-testid="text-loop"]')).toBeNull()
  })

  it('does not rotate with a single item', () => {
    stubMatchMedia(false)
    const { container } = render(<TextLoop items={['only']} intervalMs={1000} />)
    const node = () => container.querySelector('[data-testid="text-loop"]')
    expect(node()?.textContent).toBe('only')
    act(() => { vi.advanceTimersByTime(5000) })
    expect(node()?.textContent).toBe('only')
  })

  it('keeps aria-live off by default; live prop opts into polite', () => {
    stubMatchMedia(false)
    const { container } = render(<TextLoop items={['A', 'B']} />)
    expect(container.querySelector('[data-testid="text-loop"]')?.getAttribute('aria-live')).toBe('off')

    const live = render(<TextLoop items={['A', 'B']} live />)
    expect(live.container.querySelector('[data-testid="text-loop"]')?.getAttribute('aria-live')).toBe('polite')
  })

  it('keeps aria-live off under reduced motion even with live prop', () => {
    stubMatchMedia(true)
    const { container } = render(<TextLoop items={['A', 'B']} live />)
    expect(container.querySelector('[data-testid="text-loop"]')?.getAttribute('aria-live')).toBe('off')
  })
})