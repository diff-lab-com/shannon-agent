import { describe, expect, it, beforeEach, vi, afterEach } from 'vitest'
import { act, renderHook } from '@testing-library/react'
import { useReducedMotion } from '@/hooks/useReducedMotion'

interface MQListener {
  matches: boolean
  media: string
  onchange: null
  addEventListener: ReturnType<typeof vi.fn>
  removeEventListener: ReturnType<typeof vi.fn>
  dispatchEvent: ReturnType<typeof vi.fn>
  // Filled by the test to drive updates
  __trigger: (matches: boolean) => void
}

function stubMatchMedia(initial: boolean) {
  const listeners: Array<(e: { matches: boolean }) => void> = []
  const mq: MQListener = {
    matches: initial,
    media: '(prefers-reduced-motion: reduce)',
    onchange: null,
    addEventListener: vi.fn((_event: string, cb: (e: { matches: boolean }) => void) => {
      listeners.push(cb)
    }),
    removeEventListener: vi.fn((_event: string, cb: (e: { matches: boolean }) => void) => {
      const i = listeners.indexOf(cb)
      if (i >= 0) listeners.splice(i, 1)
    }),
    dispatchEvent: vi.fn(),
    __trigger: (matches: boolean) => {
      mq.matches = matches
      listeners.forEach(cb => cb({ matches }))
    },
  }
  Object.defineProperty(window, 'matchMedia', {
    writable: true,
    value: vi.fn().mockReturnValue(mq),
  })
  return mq
}

describe('useReducedMotion', () => {
  beforeEach(() => {
    // The global setup stubs matchMedia with a non-functional mock; we
    // replace it with one that lets the test drive updates.
  })

  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('returns false when matchMedia reports no reduced motion', () => {
    stubMatchMedia(false)
    const { result } = renderHook(() => useReducedMotion())
    expect(result.current).toBe(false)
  })

  it('returns true when matchMedia reports reduced motion', () => {
    stubMatchMedia(true)
    const { result } = renderHook(() => useReducedMotion())
    expect(result.current).toBe(true)
  })

  it('reacts to changes in the media query', () => {
    const mq = stubMatchMedia(false)
    const { result } = renderHook(() => useReducedMotion())
    expect(result.current).toBe(false)
    act(() => mq.__trigger(true))
    expect(result.current).toBe(true)
    act(() => mq.__trigger(false))
    expect(result.current).toBe(false)
  })

  it('removes the listener on unmount', () => {
    const mq = stubMatchMedia(false)
    const { unmount } = renderHook(() => useReducedMotion())
    expect(mq.addEventListener).toHaveBeenCalledWith('change', expect.any(Function))
    unmount()
    expect(mq.removeEventListener).toHaveBeenCalledWith('change', expect.any(Function))
  })
})