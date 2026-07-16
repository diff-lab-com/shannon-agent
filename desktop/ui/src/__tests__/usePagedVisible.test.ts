import { describe, it, expect } from 'vitest'
import { renderHook, act } from '@testing-library/react'
import { usePagedVisible } from '@/hooks/usePagedVisible'

describe('usePagedVisible', () => {
  it('returns the full list when shorter than page size', () => {
    const items = [1, 2, 3]
    const { result } = renderHook(() => usePagedVisible(items, 10))
    expect(result.current.slice).toEqual([1, 2, 3])
    expect(result.current.hasMore).toBe(false)
    expect(result.current.remaining).toBe(0)
  })

  it('slices to the initial page size when list is longer', () => {
    const items = Array.from({ length: 50 }, (_, i) => i + 1)
    const { result } = renderHook(() => usePagedVisible(items, 10))
    expect(result.current.slice).toHaveLength(10)
    expect(result.current.hasMore).toBe(true)
    expect(result.current.remaining).toBe(40)
  })

  it('showMore expands visible by one page', () => {
    const items = Array.from({ length: 50 }, (_, i) => i + 1)
    const { result } = renderHook(() => usePagedVisible(items, 10))
    act(() => result.current.showMore())
    expect(result.current.slice).toHaveLength(20)
    expect(result.current.remaining).toBe(30)
  })

  it('showMore does not overshoot the list length', () => {
    const items = Array.from({ length: 25 }, (_, i) => i + 1)
    const { result } = renderHook(() => usePagedVisible(items, 10))
    act(() => result.current.showMore())
    act(() => result.current.showMore())
    expect(result.current.slice).toHaveLength(25)
    expect(result.current.hasMore).toBe(false)
    expect(result.current.remaining).toBe(0)
  })

  it('resets to initial count when the input array identity changes', () => {
    const items = Array.from({ length: 50 }, (_, i) => i + 1)
    const { result, rerender } = renderHook(({ data }) => usePagedVisible(data, 10), {
      initialProps: { data: items },
    })
    act(() => result.current.showMore())
    expect(result.current.slice).toHaveLength(20)
    const next = Array.from({ length: 50 }, (_, i) => i + 100)
    rerender({ data: next })
    expect(result.current.slice).toHaveLength(10)
    expect(result.current.slice[0]).toBe(100)
  })

  it('honors a custom initialCount', () => {
    const items = Array.from({ length: 50 }, (_, i) => i + 1)
    const { result } = renderHook(() => usePagedVisible(items, 10, 5))
    expect(result.current.slice).toHaveLength(5)
    expect(result.current.remaining).toBe(45)
  })
})
