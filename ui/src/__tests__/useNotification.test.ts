import { describe, it, expect, vi, beforeEach } from 'vitest'
import { renderHook } from '@testing-library/react'
import { invoke } from '@tauri-apps/api/core'
import { useNotification } from '@/hooks/useNotification'

describe('useNotification', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('invokes send_notification with payload wrapper', async () => {
    const { result } = renderHook(() => useNotification())
    await result.current({ title: 'Hello', body: 'World' })
    expect(invoke).toHaveBeenCalledWith('send_notification', {
      payload: { title: 'Hello', body: 'World', level: undefined },
    })
  })

  it('passes level through when provided', async () => {
    const { result } = renderHook(() => useNotification())
    await result.current({ title: 'Boom', body: 'broken', level: 'error' })
    expect(invoke).toHaveBeenCalledWith('send_notification', {
      payload: { title: 'Boom', body: 'broken', level: 'error' },
    })
  })

  it('returns a stable callback identity across re-renders', () => {
    const { result, rerender } = renderHook(() => useNotification())
    const first = result.current
    rerender()
    expect(result.current).toBe(first)
  })
})
