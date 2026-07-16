import { describe, it, expect, vi } from 'vitest'
import { renderHook } from '@testing-library/react'
import { useTauriEvent } from '@/hooks/useTauriEvent'
import { listen } from '@tauri-apps/api/event'

describe('useTauriEvent', () => {
  it('calls listen on mount', () => {
    const handler = vi.fn()
    renderHook(() => useTauriEvent('test-event', handler))
    expect(listen).toHaveBeenCalledWith('test-event', expect.any(Function))
  })

  it('uses latest handler via ref', async () => {
    const handler1 = vi.fn()
    const handler2 = vi.fn()

    const { rerender } = renderHook(
      ({ handler }) => useTauriEvent('ref-test', handler),
      { initialProps: { handler: handler1 } }
    )

    rerender({ handler: handler2 })

    const listenerCb = (listen as ReturnType<typeof vi.fn>).mock.calls.find(
      (c: any[]) => c[0] === 'ref-test'
    )?.[1]
    if (listenerCb) {
      const mockEvent = { payload: 'test' }
      listenerCb(mockEvent)
      expect(handler2).toHaveBeenCalledWith(mockEvent)
      expect(handler1).not.toHaveBeenCalled()
    }
  })
})
