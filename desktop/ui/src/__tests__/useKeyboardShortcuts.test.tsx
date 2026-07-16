import { describe, it, expect, vi, beforeEach } from 'vitest'
import { renderHook, act } from '@testing-library/react'
import { useKeyboardShortcuts } from '@/hooks/useKeyboardShortcuts'

const mockNavigate = vi.fn()
const mockCancelQuery = vi.fn()

vi.mock('react-router-dom', () => ({
  useNavigate: () => mockNavigate,
}))

vi.mock('@/context/ChatContext', () => ({
  useChat: () => ({ cancelQuery: mockCancelQuery, isQuerying: false }),
}))

function fireKeyDown(key: string, opts: { metaKey?: boolean; ctrlKey?: boolean; shiftKey?: boolean; target?: HTMLElement } = {}) {
  const event = new KeyboardEvent('keydown', {
    key,
    metaKey: opts.metaKey ?? false,
    ctrlKey: opts.ctrlKey ?? false,
    shiftKey: opts.shiftKey ?? false,
    bubbles: true,
  })
  Object.defineProperty(event, 'target', { value: opts.target ?? document.body })
  window.dispatchEvent(event)
}

describe('useKeyboardShortcuts', () => {
  beforeEach(() => {
    mockNavigate.mockClear()
    mockCancelQuery.mockClear()
  })

  it('registers and unregisters keydown listener', () => {
    const addSpy = vi.spyOn(window, 'addEventListener')
    const removeSpy = vi.spyOn(window, 'removeEventListener')
    const { unmount } = renderHook(() => useKeyboardShortcuts())
    expect(addSpy).toHaveBeenCalledWith('keydown', expect.any(Function))
    unmount()
    expect(removeSpy).toHaveBeenCalledWith('keydown', expect.any(Function))
  })

  it('navigates to /chat on mod+n', () => {
    renderHook(() => useKeyboardShortcuts())
    fireKeyDown('n', { metaKey: true })
    expect(mockNavigate).toHaveBeenCalledWith('/chat')
  })

  it('navigates to /chat on ctrl+n', () => {
    renderHook(() => useKeyboardShortcuts())
    fireKeyDown('n', { ctrlKey: true })
    expect(mockNavigate).toHaveBeenCalledWith('/chat')
  })

  it('calls palette toggle on mod+k', () => {
    const toggle = vi.fn()
    renderHook(() => useKeyboardShortcuts(toggle))
    fireKeyDown('k', { metaKey: true })
    expect(toggle).toHaveBeenCalled()
  })

  it('dispatches shannon:change-wd event on mod+d', () => {
    const handler = vi.fn()
    window.addEventListener('shannon:change-wd', handler)
    renderHook(() => useKeyboardShortcuts())
    fireKeyDown('d', { metaKey: true })
    expect(handler).toHaveBeenCalledTimes(1)
    window.removeEventListener('shannon:change-wd', handler)
  })

  it('ignores shortcuts when focus is in input', () => {
    renderHook(() => useKeyboardShortcuts())
    const input = document.createElement('input')
    fireKeyDown('n', { metaKey: true, target: input })
    expect(mockNavigate).not.toHaveBeenCalled()
  })

  it('ignores shortcuts when focus is in textarea', () => {
    renderHook(() => useKeyboardShortcuts())
    const textarea = document.createElement('textarea')
    fireKeyDown('n', { metaKey: true, target: textarea })
    expect(mockNavigate).not.toHaveBeenCalled()
  })

  it('does not cancel query when not querying on escape', () => {
    renderHook(() => useKeyboardShortcuts())
    fireKeyDown('escape')
    expect(mockCancelQuery).not.toHaveBeenCalled()
  })
})
