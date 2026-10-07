// W10 audit §6-D — subagent banner session scoping:
//
// `subagent:start` / `subagent:stop` payloads now carry the spawning run's
// session id (backend `SubAgentEventPayload.sessionId`), and AppContext runs
// them through `isEventForCurrentWindow` — a session window must never show
// a FOREIGN session's subagent banner (it used to show every session's).
//
// Harness mirrors AppContextB1.test.tsx: a real AppProvider whose only
// replacement is a capturing @tauri-apps/api/event fake.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { renderHook, act, waitFor } from '@testing-library/react'
import { AppProvider, useApp } from '@/context/AppContext'
import { EVENT_NAMES } from '@/types'
import * as api from '@/lib/tauri-api'

const UUID_A = '7e6c3f18-4a2e-4f6a-9a52-6d1c1a0f83f1'
const UUID_B = 'deadbeef-0000-4000-8000-00000000000b'

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

function setUrlSearch(search: string) {
  window.history.replaceState(null, '', `/${search}`)
}

function wrapper({ children }: { children: React.ReactNode }) {
  return <AppProvider>{children}</AppProvider>
}

async function mountHook() {
  const utils = renderHook(() => useApp(), { wrapper })
  await waitFor(() => expect(utils.result.current.loading).toBe(false))
  await waitFor(() => {
    expect((captured[EVENT_NAMES.SUBAGENT_START] ?? []).length).toBeGreaterThan(0)
  })
  return utils
}

beforeEach(() => {
  vi.clearAllMocks()
  localStorage.clear()
  for (const key of Object.keys(captured)) delete captured[key]
  setUrlSearch('')
  vi.mocked(api.listSessions).mockResolvedValue([])
  vi.mocked(api.switchSession).mockResolvedValue([])
  vi.spyOn(console, 'warn').mockImplementation(() => undefined)
})

describe('W10 §6-D — session window drops foreign subagent banners', () => {
  it('a session window shows only its own session’s start banner', async () => {
    setUrlSearch(`?windowSession=${UUID_A}`)
    const { result } = await mountHook()
    expect(result.current.subagentLive).toBeNull()

    // Foreign session's sub-agent → dropped.
    act(() => {
      flush(EVENT_NAMES.SUBAGENT_START, {
        agentId: 'agent_b',
        agentName: 'scout-b',
        team: null,
        sessionId: UUID_B,
      })
    })
    expect(result.current.subagentLive).toBeNull()

    // Own session's sub-agent → shown.
    act(() => {
      flush(EVENT_NAMES.SUBAGENT_START, {
        agentId: 'agent_a',
        agentName: 'scout-a',
        team: null,
        sessionId: UUID_A,
      })
    })
    expect(result.current.subagentLive).toEqual({
      agentId: 'agent_a',
      agentName: 'scout-a',
      team: null,
    })
  })

  it('a session window only clears the banner on its own session’s stop', async () => {
    setUrlSearch(`?windowSession=${UUID_A}`)
    const { result } = await mountHook()
    act(() => {
      flush(EVENT_NAMES.SUBAGENT_START, {
        agentId: 'agent_a',
        agentName: 'scout-a',
        team: null,
        sessionId: UUID_A,
      })
    })
    expect(result.current.subagentLive).not.toBeNull()

    // A foreign session's stop for the same agent id must not clear ours
    // (stop stamps are filtered like starts).
    act(() => {
      flush(EVENT_NAMES.SUBAGENT_STOP, { agentId: 'agent_a', sessionId: UUID_B })
    })
    expect(result.current.subagentLive).not.toBeNull()

    act(() => {
      flush(EVENT_NAMES.SUBAGENT_STOP, { agentId: 'agent_a', sessionId: UUID_A })
    })
    expect(result.current.subagentLive).toBeNull()
  })

  it('payloads without sessionId (older backend / ambiguous stamp) keep the pre-fix every-window banner', async () => {
    setUrlSearch(`?windowSession=${UUID_A}`)
    const { result } = await mountHook()

    act(() => {
      flush(EVENT_NAMES.SUBAGENT_START, { agentId: 'agent_l', agentName: 'legacy', team: null })
    })
    expect(result.current.subagentLive).toEqual({
      agentId: 'agent_l',
      agentName: 'legacy',
      team: null,
    })
    act(() => {
      flush(EVENT_NAMES.SUBAGENT_STOP, { agentId: 'agent_l' })
    })
    expect(result.current.subagentLive).toBeNull()
  })

  it('the main window (no param) keeps accepting every session’s banners', async () => {
    const { result } = await mountHook()
    act(() => {
      flush(EVENT_NAMES.SUBAGENT_START, {
        agentId: 'agent_b',
        agentName: 'scout-b',
        team: null,
        sessionId: UUID_B,
      })
    })
    expect(result.current.subagentLive).toEqual({
      agentId: 'agent_b',
      agentName: 'scout-b',
      team: null,
    })
  })
})
