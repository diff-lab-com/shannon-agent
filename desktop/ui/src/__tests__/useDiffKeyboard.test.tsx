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

  // ---- B0 P0-4: focus gating + button passthrough ----

  /** Gated hook wired to a real container div appended to the document. */
  function renderGated(onToggle: ReturnType<typeof vi.fn>, onApply?: ReturnType<typeof vi.fn>) {
    const container = document.createElement('div')
    document.body.appendChild(container)
    const containerRef = { current: container } as React.RefObject<HTMLElement | null>
    renderHook(() =>
      useDiffKeyboard({
        enabled: true,
        hunks: computeHunks(sampleDiff.old, sampleDiff.new),
        containerRef,
        onToggleDecision: onToggle,
        onApply,
      }),
    )
    const inner = document.createElement('p')
    container.appendChild(inner)
    const fireInside = (key: string, target: EventTarget = inner) =>
      target.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }))
    return { container, fireInside }
  }

  it('ignores keys when focus is outside the gated container', () => {
    const onToggle = vi.fn()
    const { container, fireInside } = renderGated(onToggle)
    fireInside('a', document.body)
    expect(onToggle).not.toHaveBeenCalled()
    container.remove()
  })

  it('responds to keys dispatched inside the gated container', () => {
    const onToggle = vi.fn()
    const { container, fireInside } = renderGated(onToggle)
    fireInside('a')
    expect(onToggle).toHaveBeenCalledTimes(1)
    container.remove()
  })

  it('lets a focused BUTTON keep Enter instead of triggering apply', () => {
    const onApply = vi.fn()
    const { container } = renderGated(vi.fn(), onApply)
    const button = document.createElement('button')
    container.appendChild(button)
    const event = new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true })
    button.dispatchEvent(event)
    expect(onApply).not.toHaveBeenCalled()
    expect(event.defaultPrevented).toBe(false)
    container.remove()
  })

  it('ignores all shortcuts while applying', () => {
    const onToggle = vi.fn()
    const hunks = computeHunks(sampleDiff.old, sampleDiff.new)
    const { rerender } = renderHook(
      ({ applying }: { applying: boolean }) =>
        useDiffKeyboard({ enabled: true, hunks, applying, onToggleDecision: onToggle }),
      { initialProps: { applying: false } },
    )
    act(() => fireKey('a'))
    expect(onToggle).toHaveBeenCalledTimes(1)
    rerender({ applying: true })
    act(() => fireKey('a'))
    expect(onToggle).toHaveBeenCalledTimes(1)
  })
})
