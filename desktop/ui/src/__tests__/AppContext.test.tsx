import { describe, it, expect } from 'vitest'
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

    expect(spy).toHaveBeenCalledWith('Hello', undefined)
    expect(result.current.isQuerying).toBe(true)
    expect(result.current.streamingText).toBe('')
    spy.mockRestore()
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
