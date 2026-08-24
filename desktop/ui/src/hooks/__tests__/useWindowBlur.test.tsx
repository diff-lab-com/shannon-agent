import { describe, expect, it, beforeEach, vi, afterEach } from 'vitest'
import { act, renderHook } from '@testing-library/react'
import { useWindowBlur } from '@/hooks/useWindowBlur'

describe('useWindowBlur', () => {
  const originalHidden = Object.getOwnPropertyDescriptor(Document.prototype, 'hidden')

  beforeEach(() => {
    Object.defineProperty(document, 'hidden', {
      configurable: true,
      get: () => false,
    })
  })

  afterEach(() => {
    if (originalHidden) {
      Object.defineProperty(Document.prototype, 'hidden', originalHidden)
    }
    vi.restoreAllMocks()
  })

  it('returns false when document is visible', () => {
    Object.defineProperty(document, 'hidden', { configurable: true, get: () => false })
    const { result } = renderHook(() => useWindowBlur())
    expect(result.current).toBe(false)
  })

  it('returns true when document is hidden', () => {
    Object.defineProperty(document, 'hidden', { configurable: true, get: () => true })
    const { result } = renderHook(() => useWindowBlur())
    expect(result.current).toBe(true)
  })

  it('reacts to visibilitychange events', () => {
    let hidden = false
    Object.defineProperty(document, 'hidden', {
      configurable: true,
      get: () => hidden,
    })
    const { result } = renderHook(() => useWindowBlur())
    expect(result.current).toBe(false)

    act(() => {
      hidden = true
      document.dispatchEvent(new Event('visibilitychange'))
    })
    expect(result.current).toBe(true)

    act(() => {
      hidden = false
      document.dispatchEvent(new Event('visibilitychange'))
    })
    expect(result.current).toBe(false)
  })

  it('removes the visibilitychange listener on unmount', () => {
    const removeSpy = vi.spyOn(document, 'removeEventListener')
    const { unmount } = renderHook(() => useWindowBlur())
    unmount()
    expect(removeSpy).toHaveBeenCalledWith('visibilitychange', expect.any(Function))
  })
})