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

  // P0-4 final-review fix: a budget-rejected send must roll back its
  // optimistic append, so "Continue (ignore once)" re-sending the same text
  // renders the message exactly once — never twice.
  it('rolls back the optimistic user message when the send is rejected', async () => {
    vi.spyOn(api, 'sendMessage').mockRejectedValue(
      new Error(
        'Session budget exceeded: spent $1.0000 of $1.0000 — continue (ignore once), raise the budget, or stop',
      ),
    )
    const { result } = renderHook(() => useApp(), { wrapper })
    // Let the initial data load settle first so its setMessages calls can
    // never interleave with (and clobber) the sends under test.
    await waitFor(() => expect(result.current.loading).toBe(false))

    await act(async () => {
      await result.current.sendMessage('Hello')
    })

    expect(result.current.error).toContain('Session budget exceeded')
    expect(result.current.isQuerying).toBe(false)
    // The optimistic append was rolled back — the rejected message is gone.
    expect(result.current.messages.filter(m => m.role === 'user' && m.content === 'Hello')).toHaveLength(0)

    // "Continue (ignore once)": the same text is re-sent with the bypass
    // flag and accepted — it renders exactly once (no duplicate).
    const bypassed = vi.spyOn(api, 'sendMessage').mockResolvedValue({ query_id: 'q2' })
    await act(async () => {
      await result.current.sendMessage('Hello', undefined, { budgetBypass: true })
    })
    expect(bypassed).toHaveBeenCalledWith('Hello', undefined, true, undefined)
    expect(result.current.messages.filter(m => m.role === 'user' && m.content === 'Hello')).toHaveLength(1)
    bypassed.mockRestore()
  })

  it('keeps the optimistic user message when the send is accepted', async () => {
    const spy = vi.spyOn(api, 'sendMessage').mockResolvedValue({ query_id: 'q1' })
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))

    await act(async () => {
      await result.current.sendMessage('Hello')
    })

    expect(result.current.messages.filter(m => m.role === 'user' && m.content === 'Hello')).toHaveLength(1)
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
