// B1-2 (P1-1) — permission prompt lifecycle follows the run's terminal
// states: completed/failed/cancelled must dismiss a still-pending approval
// dialog for the SAME session (and clear the rail's amber dot); another
// session's terminal event must not.
//
// Harness mirrors AppContextB1.test.tsx: a real AppProvider whose only
// replacement is a capturing @tauri-apps/api/event fake — the terminal
// handlers are exercised by flushing events straight at the captured
// listeners (listener-level seam; the state-machine harness does not own
// these paths).

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { renderHook, act, waitFor } from '@testing-library/react'
import { AppProvider, useApp } from '@/context/AppContext'
import { EVENT_NAMES } from '@/types'
import * as api from '@/lib/tauri-api'

const SESSION_A = 'aaaa1111-0000-4000-8000-00000000000a'
const SESSION_B = 'bbbb2222-0000-4000-8000-00000000000b'

const { captured, flush } = vi.hoisted(() => {
  const captured: Record<string, ((e: { payload: unknown }) => void)[]> = {}
  const flush = (event: string, payload: unknown) => {
    for (const h of captured[event] ?? []) h(payload)
  }
  return { captured, flush }
})

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn((event: string, handler: (e: { payload: unknown }) => void) => {
    captured[event] ??= []
    captured[event].push((payload) => handler({ payload }))
    return Promise.resolve(() => {
      captured[event] = (captured[event] ?? []).filter(h => h !== handler)
    })
  }),
  emit: vi.fn(),
}))

function wrapper({ children }: { children: React.ReactNode }) {
  return <AppProvider>{children}</AppProvider>
}

async function flushUntilRegistered() {
  await waitFor(() => {
    expect((captured[EVENT_NAMES.QUERY_TEXT] ?? []).length).toBeGreaterThan(0)
  })
}

const promptFor = (sessionId: string | undefined, requestId = 'req-1') => ({
  tool: 'bash',
  input: { command: 'rm -rf /tmp/x' },
  risk: 'high',
  request_id: requestId,
  ...(sessionId !== undefined ? { session_id: sessionId } : {}),
})

beforeEach(() => {
  vi.clearAllMocks()
  localStorage.clear()
  for (const key of Object.keys(captured)) delete captured[key]
  vi.mocked(api.listSessions).mockResolvedValue([])
  vi.mocked(api.newSession).mockResolvedValue(SESSION_A)
  vi.mocked(api.switchSession).mockResolvedValue([])
})

describe('B1-2 (P1-1) — run terminal states dismiss a pending approval prompt', () => {
  it('QUERY_CANCELLED for the prompting session clears the dialog and the rail dot', async () => {
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    await flushUntilRegistered()
    await act(async () => { await result.current.createSession() })

    // Approval dialog pending on A (rail's amber dot lit).
    act(() => { flush(EVENT_NAMES.PERMISSION_REQUEST, promptFor(SESSION_A)) })
    expect(result.current.permissionRequest?.request_id).toBe('req-1')
    expect(result.current.sessionActivity[SESSION_A]?.awaitingApproval).toBe(true)

    // The user stops the run while the dialog is up — the ghost prompt must
    // not survive (the backend auto-Denies it after its 300s timeout).
    act(() => { flush(EVENT_NAMES.QUERY_CANCELLED, { session_id: SESSION_A }) })
    expect(result.current.permissionRequest).toBeNull()
    expect(result.current.sessionActivity[SESSION_A]?.awaitingApproval).toBe(false)
  })

  it("another session's terminal event leaves the prompt and dot alone", async () => {
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    await flushUntilRegistered()
    await act(async () => { await result.current.createSession() })

    act(() => { flush(EVENT_NAMES.PERMISSION_REQUEST, promptFor(SESSION_A)) })

    // B settling (any terminal kind) must not dismiss A's dialog.
    act(() => { flush(EVENT_NAMES.QUERY_CANCELLED, { session_id: SESSION_B }) })
    act(() => { flush(EVENT_NAMES.QUERY_COMPLETED, { session_id: SESSION_B }) })
    act(() => { flush(EVENT_NAMES.QUERY_FAILED, { error: 'boom-b', session_id: SESSION_B }) })
    expect(result.current.permissionRequest?.request_id).toBe('req-1')
    expect(result.current.sessionActivity[SESSION_A]?.awaitingApproval).toBe(true)

    // Only A's own terminal resolves it.
    act(() => { flush(EVENT_NAMES.QUERY_CANCELLED, { session_id: SESSION_A }) })
    expect(result.current.permissionRequest).toBeNull()
    expect(result.current.sessionActivity[SESSION_A]?.awaitingApproval).toBe(false)
  })

  it('QUERY_COMPLETED and QUERY_FAILED dismiss the prompt the same way', async () => {
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    await flushUntilRegistered()
    await act(async () => { await result.current.createSession() })

    act(() => { flush(EVENT_NAMES.PERMISSION_REQUEST, promptFor(SESSION_A, 'req-1')) })
    act(() => { flush(EVENT_NAMES.QUERY_COMPLETED, { session_id: SESSION_A }) })
    expect(result.current.permissionRequest).toBeNull()
    expect(result.current.sessionActivity[SESSION_A]?.awaitingApproval).toBe(false)

    // The next prompt of the session starts clean and settles the same way.
    act(() => { flush(EVENT_NAMES.PERMISSION_REQUEST, promptFor(SESSION_A, 'req-2')) })
    expect(result.current.permissionRequest?.request_id).toBe('req-2')
    expect(result.current.sessionActivity[SESSION_A]?.awaitingApproval).toBe(true)
    act(() => { flush(EVENT_NAMES.QUERY_FAILED, { error: 'boom', session_id: SESSION_A }) })
    expect(result.current.permissionRequest).toBeNull()
    expect(result.current.sessionActivity[SESSION_A]?.awaitingApproval).toBe(false)
  })

  it('a session-less prompt (request_permission command path) is never dismissed by a terminal event', async () => {
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    await flushUntilRegistered()
    await act(async () => { await result.current.createSession() })

    // No session_id on the payload — it cannot be attributed to a session,
    // so a chat run settling must leave it up for its own dialog flow.
    act(() => { flush(EVENT_NAMES.PERMISSION_REQUEST, promptFor(undefined)) })
    expect(result.current.permissionRequest?.request_id).toBe('req-1')

    act(() => { flush(EVENT_NAMES.QUERY_CANCELLED, { session_id: SESSION_A }) })
    act(() => { flush(EVENT_NAMES.QUERY_COMPLETED, { session_id: SESSION_A }) })
    expect(result.current.permissionRequest?.request_id).toBe('req-1')
  })
})
