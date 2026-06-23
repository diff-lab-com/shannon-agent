import { describe, it, expect, vi, beforeEach } from 'vitest'
import { renderHook } from '@testing-library/react'
import { useTauriEventValidated } from '../useTauriEventValidated'

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn((_event: string, cb: (e: { payload: unknown }) => void) => {
    ;(globalThis as { __tauriListenMock?: (payload: unknown) => void }).__tauriListenMock = (
      payload: unknown,
    ) => cb({ payload, event: _event, id: 0 })
    return Promise.resolve(() => {})
  }),
}))

describe('useTauriEventValidated', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.spyOn(console, 'warn').mockImplementation(() => {})
  })

  it('invokes handler when valid payload arrives', async () => {
    const handler = vi.fn()
    renderHook(() => useTauriEventValidated('query:text', handler))
    const emit = (globalThis as { __tauriListenMock?: (p: unknown) => void }).__tauriListenMock
    emit?.({ query_id: 'q1', content: 'hello' })
    expect(handler).toHaveBeenCalledTimes(1)
    expect(console.warn).not.toHaveBeenCalled()
  })

  it('warns on schema mismatch (dev mode) but still invokes handler', async () => {
    const handler = vi.fn()
    renderHook(() => useTauriEventValidated('query:text', handler))
    const emit = (globalThis as { __tauriListenMock?: (p: unknown) => void }).__tauriListenMock
    emit?.({ query_id: 'q1' })
    expect(handler).toHaveBeenCalledTimes(1)
  })

  it('no-op validation for unmapped event names', async () => {
    const handler = vi.fn()
    renderHook(() => useTauriEventValidated('unknown-event', handler))
    const emit = (globalThis as { __tauriListenMock?: (p: unknown) => void }).__tauriListenMock
    emit?.({ anything: true })
    expect(handler).toHaveBeenCalledTimes(1)
    expect(console.warn).not.toHaveBeenCalled()
  })
})
