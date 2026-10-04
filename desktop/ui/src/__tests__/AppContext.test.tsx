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

  // A-11 fix: two identical texts in flight at once (double-Enter before
  // isQuerying flips, drain vs manual send) — the first send's FAILURE must
  // roll back only its own optimistic bubble. The old role+content matcher
  // deleted the LAST same-text message, i.e. the second send's bubble,
  // while the failed one stayed on screen.
  it('rolls back only the failed send when two identical sends race (A-11)', async () => {
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    const sendSpy = vi.spyOn(api, 'sendMessage')
      .mockRejectedValueOnce(new Error('another query is already running for this session'))
      .mockResolvedValueOnce({ query_id: 'q2' })
    let first!: Promise<boolean>
    let second!: Promise<boolean>
    await act(async () => {
      // Fired without awaiting either: both optimistic bubbles exist before
      // either send settles.
      first = result.current.sendMessage('Hello')
      second = result.current.sendMessage('Hello', ['/tmp/a.txt'])
    })
    await act(async () => { await Promise.all([first, second]) })

    const hellos = result.current.messages.filter(m => m.role === 'user' && m.content === 'Hello')
    expect(hellos).toHaveLength(1)
    // The survivor is the SECOND send's bubble (the one that succeeded) —
    // its attachment paths ride along; content matching would have kept the
    // attachmentless failed one instead.
    expect(hellos[0]!.file_attachments).toEqual([
      { name: 'a.txt', path: '/tmp/a.txt', size: 0 },
    ])
    sendSpy.mockRestore()
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

  // ── R4 group 3 — session binding ────────────────────────────────────────

  // A-5 fix: the error banner is a visible-session readout, but nothing
  // cleared it when the user moved to another session — the previous
  // session's failure banner (auth included) followed them there.
  it('clears the chat error when the user switches to another session (A-5)', async () => {
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))

    // A rejected send leaves its error on screen…
    const sendSpy = vi.spyOn(api, 'sendMessage').mockRejectedValue(new Error('401 Unauthorized'))
    await act(async () => {
      await result.current.sendMessage('Hello')
    })
    expect(result.current.error).toBe('Error: 401 Unauthorized')
    sendSpy.mockRestore()

    // …and switching sessions must not carry it over.
    const switchSpy = vi.spyOn(api, 'switchSession').mockResolvedValue([])
    await act(async () => {
      await result.current.switchSession('11111111-2222-4333-8444-555555555555')
    })
    expect(result.current.error).toBeNull()
    // Kept in lockstep with `error` by the single writer.
    expect(result.current.errorKind).toBeNull()
    switchSpy.mockRestore()
  })

  // A-5 semantics pin: a SAME-session reload (Chat remount re-running the
  // switch against the current id) belongs to the session still on screen —
  // its error banner must survive the reload.
  it('keeps the error banner on a same-session reload (A-5)', async () => {
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))

    const switchSpy = vi.spyOn(api, 'switchSession').mockResolvedValue([])
    await act(async () => {
      await result.current.switchSession('11111111-2222-4333-8444-555555555555')
    })
    const sendSpy = vi.spyOn(api, 'sendMessage').mockRejectedValue(new Error('boom'))
    await act(async () => {
      await result.current.sendMessage('Hello')
    })
    expect(result.current.error).toBe('Error: boom')

    await act(async () => {
      await result.current.switchSession('11111111-2222-4333-8444-555555555555')
    })
    expect(result.current.error).toBe('Error: boom')
    switchSpy.mockRestore()
    sendSpy.mockRestore()
  })

  // A-6 fix: the cold-start conversation comes from the backend's ACTIVE
  // session, but `currentSessionId` stayed null — RunStatusLine had no
  // session to time against and every currentSessionId-gated action stayed
  // half-bound until the first manual switch.
  it('binds currentSessionId to the active session on cold start (A-6)', async () => {
    const convSpy = vi.spyOn(api, 'getConversation').mockResolvedValue([
      { role: 'user', content: 'recorded history', timestamp: 1 },
    ])
    const activeSpy = vi.spyOn(api, 'getActiveSessionId').mockResolvedValue('aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee')
    const { result } = renderHook(() => useApp(), { wrapper })

    await waitFor(() => expect(result.current.loading).toBe(false))

    expect(result.current.messages).toEqual([
      { role: 'user', content: 'recorded history', timestamp: 1 },
    ])
    expect(result.current.currentSessionId).toBe('aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee')

    // The bound id is the composer's explicit routing target from the very
    // first send (P1-1's explicit-session path, no null fallback).
    const sendSpy = vi.spyOn(api, 'sendMessage').mockResolvedValue({ query_id: 'q1' })
    await act(async () => {
      await result.current.sendMessage('Hello')
    })
    expect(sendSpy).toHaveBeenCalledWith('Hello', undefined, undefined, 'aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee')
    sendSpy.mockRestore()
    convSpy.mockRestore()
    activeSpy.mockRestore()
  })

  // A-6 guard: when the backend reports no active session (the mock's demo
  // world), the cold start stays unbound — no fabricated id.
  it('leaves currentSessionId null when the backend has no active session (A-6)', async () => {
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    expect(result.current.currentSessionId).toBeNull()
  })

  // P2-8: a failed worktree bind must not strand the `new_session` the call
  // already made — the backend session is deleted best-effort (a failed
  // delete never masks the original worktree error), and the frontend
  // pointer rolls back as before.
  describe('createSessionInWorktree orphan rollback (P2-8)', () => {
    const ORPHAN_ID = 'c0c0c0c0-0000-4000-8000-00000000c0c0'

    async function renderAndCreate() {
      const { result } = renderHook(() => useApp(), { wrapper })
      await waitFor(() => expect(result.current.loading).toBe(false))
      await act(async () => { await result.current.createSessionInWorktree() })
      return result
    }

    beforeEach(() => {
      vi.mocked(api.newSession).mockResolvedValue(ORPHAN_ID)
      vi.mocked(api.createSessionWorktree).mockRejectedValue(new Error('git worktree add failed'))
    })

    it('deletes the backend session when worktree creation fails', async () => {
      const deleteSpy = vi.spyOn(api, 'deleteSession').mockResolvedValue(true)
      const result = await renderAndCreate()
      expect(deleteSpy).toHaveBeenCalledWith(ORPHAN_ID)
      // The original worktree error is what surfaces — not the rollback.
      expect(result.current.error).toContain('git worktree add failed')
      // ...and the composer pointer rolled back (unchanged behavior).
      expect(result.current.currentSessionId).toBeNull()
      deleteSpy.mockRestore()
    })

    it('a failing rollback delete is logged, never masks the worktree error', async () => {
      const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})
      const deleteSpy = vi.spyOn(api, 'deleteSession').mockRejectedValue(new Error('delete failed too'))
      const result = await renderAndCreate()
      expect(deleteSpy).toHaveBeenCalledWith(ORPHAN_ID)
      expect(result.current.error).toContain('git worktree add failed')
      expect(result.current.error).not.toContain('delete failed too')
      expect(warnSpy).toHaveBeenCalledWith(expect.stringContaining('orphan session'), expect.any(Error))
      warnSpy.mockRestore()
      deleteSpy.mockRestore()
    })
  })

})
