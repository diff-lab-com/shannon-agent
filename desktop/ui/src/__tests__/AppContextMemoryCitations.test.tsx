// W3-4 — the per-turn injected-memory snapshot riding the send response is
// attached to the committed assistant message as citation chips data.
//
// The snapshot is stashed per owning session at send time, popped onto the
// assistant message at QUERY_COMPLETED, and dropped on fail/cancel so it can
// never leak onto a later turn's bubble.

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

beforeEach(() => {
  vi.clearAllMocks()
  for (const key of Object.keys(captured)) delete captured[key]
  vi.mocked(api.listSessions).mockResolvedValue([])
  vi.mocked(api.newSession).mockResolvedValue(SESSION_A)
  vi.mocked(api.switchSession).mockResolvedValue([])
})

const citations = [
  { id: 'm1', title: 'use pnpm not npm', category: 'preference', sourceSessionId: 's0' },
]

async function setupStreamingSession(result: ReturnType<typeof useApp>) {
  await act(async () => { await result.current.createSession() })
  await act(async () => { await result.current.sendMessage('Hello') })
  act(() => {
    flush(EVENT_NAMES.QUERY_TEXT, { content: 'answer', session_id: SESSION_A })
  })
  await waitFor(() => expect(result.current.streamingText).toBe('answer'))
}

describe('AppContext — W3-4 per-turn memory citation snapshot', () => {
  it('attaches the snapshot to the committed assistant message', async () => {
    vi.mocked(api.sendMessage).mockResolvedValue({ query_id: 'q1', injected_memories: citations })
    const result = renderHook(() => useApp(), { wrapper }).result
    await waitFor(() => expect(result.current.loading).toBe(false))
    await setupStreamingSession(result)

    act(() => { flush(EVENT_NAMES.QUERY_COMPLETED, { session_id: SESSION_A }) })

    const assistants = result.current.messages.filter(m => m.role === 'assistant')
    expect(assistants).toHaveLength(1)
    expect(assistants[0].injected_memories).toEqual(citations)
  })

  it('omits the field for an empty snapshot (bypass / zero injections)', async () => {
    vi.mocked(api.sendMessage).mockResolvedValue({ query_id: 'q1', injected_memories: [] })
    const result = renderHook(() => useApp(), { wrapper }).result
    await waitFor(() => expect(result.current.loading).toBe(false))
    await setupStreamingSession(result)

    act(() => { flush(EVENT_NAMES.QUERY_COMPLETED, { session_id: SESSION_A }) })

    const assistants = result.current.messages.filter(m => m.role === 'assistant')
    expect(assistants).toHaveLength(1)
    expect(assistants[0].injected_memories).toBeUndefined()
  })

  it('drops the snapshot when the run fails — no leak onto a later turn', async () => {
    vi.mocked(api.sendMessage).mockResolvedValue({ query_id: 'q1', injected_memories: citations })
    const result = renderHook(() => useApp(), { wrapper }).result
    await waitFor(() => expect(result.current.loading).toBe(false))
    await setupStreamingSession(result)

    act(() => { flush(EVENT_NAMES.QUERY_FAILED, { error: 'boom', session_id: SESSION_A }) })
    expect(result.current.messages.filter(m => m.role === 'assistant')).toHaveLength(0)

    // The next turn commits WITHOUT the failed turn's citations.
    vi.mocked(api.sendMessage).mockResolvedValue({ query_id: 'q2', injected_memories: [] })
    await act(async () => { await result.current.sendMessage('again') })
    act(() => {
      flush(EVENT_NAMES.QUERY_TEXT, { content: 'second answer', session_id: SESSION_A })
    })
    act(() => { flush(EVENT_NAMES.QUERY_COMPLETED, { session_id: SESSION_A }) })

    const assistants = result.current.messages.filter(m => m.role === 'assistant')
    expect(assistants).toHaveLength(1)
    expect(assistants[0].injected_memories).toBeUndefined()
  })

  it('keys the snapshot per session — a background completion never lands on the visible session', async () => {
    vi.mocked(api.sendMessage).mockResolvedValue({ query_id: 'q1', injected_memories: citations })
    const result = renderHook(() => useApp(), { wrapper }).result
    await waitFor(() => expect(result.current.loading).toBe(false))
    await act(async () => { await result.current.createSession() })

    // Session B streams in the background with its own response snapshot.
    vi.mocked(api.sendMessage).mockResolvedValue({ query_id: 'qB', injected_memories: citations })
    // Send targets the visible session A; simulate B's response stash by
    // sending from B's perspective via a windowSessionId-less main-window
    // context: only A's send exists here, so B completing must not attach.
    await act(async () => { await result.current.sendMessage('Hello') })

    act(() => {
      flush(EVENT_NAMES.QUERY_TEXT, { content: 'A text', session_id: SESSION_A })
      flush(EVENT_NAMES.QUERY_COMPLETED, { session_id: SESSION_B })
    })
    const assistantsA = result.current.messages.filter(m => m.role === 'assistant')
    expect(assistantsA).toHaveLength(0, 'B completing must not commit into A')
  })
})
