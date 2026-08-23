import { describe, expect, it, vi } from 'vitest'
import { render } from '@testing-library/react'
import { MotionGuard } from '@/components/ui/MotionGuard'

describe('MotionGuard', () => {
  it('renders children when active and motion is allowed', () => {
    const { container } = render(
      <MotionGuard active>
        <span data-testid="animated">animated</span>
      </MotionGuard>,
    )
    expect(container.querySelector('[data-testid="animated"]')).not.toBeNull()
  })

  it('renders fallback when active=false', () => {
    const { container } = render(
      <MotionGuard active={false} fallback={<span data-testid="static">static</span>}>
        <span data-testid="animated">animated</span>
      </MotionGuard>,
    )
    expect(container.querySelector('[data-testid="animated"]')).toBeNull()
    expect(container.querySelector('[data-testid="static"]')).not.toBeNull()
  })

  it('renders fallback when matchMedia reports reduced motion', () => {
    // Global setup stubs matchMedia to non-functional; replace it here.
    Object.defineProperty(window, 'matchMedia', {
      writable: true,
      value: vi.fn().mockImplementation((query: string) => ({
        matches: true,
        media: query,
        onchange: null,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
        dispatchEvent: vi.fn(),
      })),
    })
    const { container } = render(
      <MotionGuard active fallback={<span data-testid="static">static</span>}>
        <span data-testid="animated">animated</span>
      </MotionGuard>,
    )
    expect(container.querySelector('[data-testid="animated"]')).toBeNull()
    expect(container.querySelector('[data-testid="static"]')).not.toBeNull()
  })

  it('renders children when no fallback is provided even if motion is off', () => {
    const { container } = render(
      <MotionGuard active={false}>
        <span data-testid="animated">animated</span>
      </MotionGuard>,
    )
    // active=false still short-circuits to children when no fallback given
    expect(container.querySelector('[data-testid="animated"]')).not.toBeNull()
  })
})