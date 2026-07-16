import { describe, it, expect, vi } from 'vitest'
import { renderHook, act } from '@testing-library/react'
import { useDiffKeyboard } from '@/hooks/useDiffKeyboard'
import { computeHunks } from '@/lib/diff-merge'

const sampleDiff = {
  old: 'line one\nline two\nline three',
  new: 'line one\nline two edited\nline three\nline four',
}

function fireKey(key: string) {
  const event = new KeyboardEvent('keydown', { key, bubbles: true })
  document.dispatchEvent(event)
  return event
}

describe('useDiffKeyboard', () => {
  it('does nothing when disabled', () => {
    const onToggle = vi.fn()
    const hunks = computeHunks(sampleDiff.old, sampleDiff.new)
    renderHook(() =>
      useDiffKeyboard({ enabled: false, hunks, onToggleDecision: onToggle }),
    )
    fireKey('a')
    expect(onToggle).not.toHaveBeenCalled()
  })

  it('j / ArrowDown advances to next hunk', () => {
    const onToggle = vi.fn()
    const hunks = computeHunks(sampleDiff.old, sampleDiff.new)
    const { result } = renderHook(() =>
      useDiffKeyboard({ enabled: true, hunks, onToggleDecision: onToggle }),
    )
    expect(result.current.currentHunkId).toBe(hunks[0].id)
    act(() => fireKey('j'))
    expect(result.current.currentHunkId).toBe(hunks[1].id)
  })

  it('k / ArrowUp wraps around', () => {
    const onToggle = vi.fn()
    const hunks = computeHunks(sampleDiff.old, sampleDiff.new)
    const { result } = renderHook(() =>
      useDiffKeyboard({ enabled: true, hunks, onToggleDecision: onToggle }),
    )
    expect(result.current.currentHunkId).toBe(hunks[0].id)
    act(() => fireKey('k'))
    expect(result.current.currentHunkId).toBe(hunks[hunks.length - 1].id)
  })

  it('a accepts the current hunk', () => {
    const onToggle = vi.fn()
    const hunks = computeHunks(sampleDiff.old, sampleDiff.new)
    renderHook(() =>
      useDiffKeyboard({ enabled: true, hunks, onToggleDecision: onToggle }),
    )
    act(() => fireKey('a'))
    expect(onToggle).toHaveBeenCalledWith(hunks[0].id, 'accept')
  })

  it('r rejects the current hunk', () => {
    const onToggle = vi.fn()
    const hunks = computeHunks(sampleDiff.old, sampleDiff.new)
    renderHook(() =>
      useDiffKeyboard({ enabled: true, hunks, onToggleDecision: onToggle }),
    )
    act(() => fireKey('r'))
    expect(onToggle).toHaveBeenCalledWith(hunks[0].id, 'reject')
  })

  it('u resets the current hunk to pending', () => {
    const onToggle = vi.fn()
    const hunks = computeHunks(sampleDiff.old, sampleDiff.new)
    renderHook(() =>
      useDiffKeyboard({ enabled: true, hunks, onToggleDecision: onToggle }),
    )
    act(() => fireKey('u'))
    expect(onToggle).toHaveBeenCalledWith(hunks[0].id, 'pending')
  })

  it('Enter invokes onApply when provided', () => {
    const onApply = vi.fn()
    const hunks = computeHunks(sampleDiff.old, sampleDiff.new)
    renderHook(() =>
      useDiffKeyboard({ enabled: true, hunks, onToggleDecision: vi.fn(), onApply }),
    )
    act(() => fireKey('Enter'))
    expect(onApply).toHaveBeenCalledTimes(1)
  })

  it('setCurrentHunkId moves the cursor', () => {
    const onToggle = vi.fn()
    const hunks = computeHunks(sampleDiff.old, sampleDiff.new)
    const { result } = renderHook(() =>
      useDiffKeyboard({ enabled: true, hunks, onToggleDecision: onToggle }),
    )
    act(() => result.current.setCurrentHunkId(hunks[1].id))
    expect(result.current.currentHunkId).toBe(hunks[1].id)
  })

  it('ignores keys when typing in an input', () => {
    const onToggle = vi.fn()
    const hunks = computeHunks(sampleDiff.old, sampleDiff.new)
    renderHook(() =>
      useDiffKeyboard({ enabled: true, hunks, onToggleDecision: onToggle }),
    )
    const input = document.createElement('input')
    document.body.appendChild(input)
    input.focus()
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'a', bubbles: true }))
    expect(onToggle).not.toHaveBeenCalled()
    document.body.removeChild(input)
  })
})
