import { describe, it, expect, vi } from 'vitest'
import { renderHook, waitFor, act } from '@testing-library/react'
import { AppProvider, useApp } from '@/context/AppContext'
import * as api from '@/lib/tauri-api'

function wrapper({ children }: { children: React.ReactNode }) {
  return <AppProvider>{children}</AppProvider>
}

describe('AppContext', () => {
  it('provides initial state', () => {
    const { result } = renderHook(() => useApp(), { wrapper })
    expect(result.current.messages).toEqual([])
    expect(result.current.streamingText).toBe('')
    expect(result.current.isQuerying).toBe(false)
    expect(result.current.error).toBeNull()
    expect(result.current.permissionRequest).toBeNull()
  })

  it('provides all required actions', () => {
    const { result } = renderHook(() => useApp(), { wrapper })
    expect(typeof result.current.sendMessage).toBe('function')
    expect(typeof result.current.cancelQuery).toBe('function')
    expect(typeof result.current.createSession).toBe('function')
    expect(typeof result.current.switchSession).toBe('function')
    expect(typeof result.current.deleteSession).toBe('function')
    expect(typeof result.current.renameSession).toBe('function')
    expect(typeof result.current.respondPermission).toBe('function')
    expect(typeof result.current.refreshSessions).toBe('function')
    expect(typeof result.current.refreshStatus).toBe('function')
    expect(typeof result.current.refreshModels).toBe('function')
  })

  it('loads status on mount', async () => {
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => {
      expect(result.current.status).not.toBeNull()
    })
    expect(result.current.status?.model).toBe('claude-sonnet-4-6')
  })

  it('loads models on mount', async () => {
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => {
      expect(result.current.models.length).toBeGreaterThan(0)
    })
  })

  it('sendMessage sets querying state and calls api', async () => {
    const spy = vi.spyOn(api, 'sendMessage').mockResolvedValue({ query_id: 'q1' })
    const { result } = renderHook(() => useApp(), { wrapper })

    await act(async () => {
      await result.current.sendMessage('Hello')
    })

    // P0-4: the third arg carries the optional budget-bypass flag.
    // P1-1 fix: the fourth arg routes explicitly — no currentSessionId in
    // this bare render, so undefined keeps the backend active fallback.
    expect(spy).toHaveBeenCalledWith('Hello', undefined, undefined, undefined)
    expect(result.current.isQuerying).toBe(true)
    expect(result.current.streamingText).toBe('')
    spy.mockRestore()
  })

  it('sendMessage routes to the current session explicitly (P1-1 fix)', async () => {
    const newId = '11111111-2222-4333-8444-555555555555'
    vi.spyOn(api, 'newSession').mockResolvedValue(newId)
    const sendSpy = vi.spyOn(api, 'sendMessage').mockResolvedValue({ query_id: 'q1' })
    const { result } = renderHook(() => useApp(), { wrapper })

    await act(async () => {
      await result.current.createSession()
    })
    expect(result.current.currentSessionId).toBe(newId)

    await act(async () => {
      await result.current.sendMessage('Hello')
    })

    // The main window names its own active session on every send — the
    // backend routes via the explicit id, never the shared pointer.
    expect(sendSpy).toHaveBeenCalledWith('Hello', undefined, undefined, newId)
    sendSpy.mockRestore()
  })

  it('cancelQuery routes to the current session explicitly (P1-1 fix)', async () => {
    const newId = '11111111-2222-4333-8444-555555555555'
    vi.spyOn(api, 'newSession').mockResolvedValue(newId)
    const cancelSpy = vi.spyOn(api, 'cancelQuery').mockResolvedValue(undefined)
    const { result } = renderHook(() => useApp(), { wrapper })

    await act(async () => {
      await result.current.createSession()
    })
    expect(result.current.currentSessionId).toBe(newId)

    await act(async () => {
      await result.current.cancelQuery()
    })

    expect(cancelSpy).toHaveBeenCalledWith(newId)
    cancelSpy.mockRestore()
  })

  it('sendMessage handles errors', async () => {
    const spy = vi.spyOn(api, 'sendMessage').mockRejectedValue(new Error('API error'))
    const { result } = renderHook(() => useApp(), { wrapper })

    await act(async () => {
      await result.current.sendMessage('Hello')
    })

    expect(result.current.error).toBe('Error: API error')
    expect(result.current.isQuerying).toBe(false)
    spy.mockRestore()
  })

  it('cancelQuery calls api', async () => {
    const spy = vi.spyOn(api, 'cancelQuery').mockResolvedValue(undefined)
    const { result } = renderHook(() => useApp(), { wrapper })

    await act(async () => {
      await result.current.cancelQuery()
    })

    expect(spy).toHaveBeenCalled()
    spy.mockRestore()
  })

  it('createSession resets state and calls api', async () => {
    const spy = vi.spyOn(api, 'newSession').mockResolvedValue('new-id')
    const { result } = renderHook(() => useApp(), { wrapper })

    await act(async () => {
      await result.current.createSession()
    })

    expect(spy).toHaveBeenCalled()
    expect(result.current.currentSessionId).toBe('new-id')
    expect(result.current.messages).toEqual([])
    spy.mockRestore()
  })
})
