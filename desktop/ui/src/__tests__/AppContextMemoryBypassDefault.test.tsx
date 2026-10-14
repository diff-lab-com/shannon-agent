// design-parity R2 (M3b, design 12-settings-general.html:224-232) — the
// 记忆旁路默认 pref (lib/memoryBypassPref, localStorage) must reach REAL
// sessions: AppContext.createSession / createSessionInWorktree pin each
// just-created session to temporary chat via setSessionMemoryBypass — the
// same per-session pin the composer's toggle writes (sidecar-persisted,
// backend-side), applied BEFORE the first send so the memory layer never
// sees the session. Off (the default) is the status quo: no pin at all.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { renderHook, act } from '@testing-library/react'
import { AppProvider, useApp } from '@/context/AppContext'
import * as api from '@/lib/tauri-api'

function wrapper({ children }: { children: React.ReactNode }) {
  return <AppProvider>{children}</AppProvider>
}

const PREF_KEY = 'shannon.chat.memoryBypassDefault'

describe('AppContext createSession applies the 记忆旁路默认 pref (design-parity R2 M3b)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    window.localStorage.clear()
    vi.mocked(api.newSession).mockResolvedValue('session-new-1')
    vi.mocked(api.listSessions).mockResolvedValue([])
    vi.mocked(api.setSessionMemoryBypass).mockResolvedValue(undefined)
  })

  it('pref ON: a just-created session is pinned to temporary chat', async () => {
    window.localStorage.setItem(PREF_KEY, 'true')
    const { result } = renderHook(() => useApp(), { wrapper })
    await act(async () => { await result.current.createSession() })
    expect(api.setSessionMemoryBypass).toHaveBeenCalledWith('session-new-1', true)
  })

  it('pref OFF (the default): new sessions use memory — no pin at all', async () => {
    const { result } = renderHook(() => useApp(), { wrapper })
    await act(async () => { await result.current.createSession() })
    expect(api.setSessionMemoryBypass).not.toHaveBeenCalled()
  })

  it('a failed sidecar write does not fail the session creation (best-effort)', async () => {
    window.localStorage.setItem(PREF_KEY, 'true')
    vi.mocked(api.setSessionMemoryBypass).mockRejectedValueOnce('sidecar write refused')
    const { result } = renderHook(() => useApp(), { wrapper })
    await act(async () => { await result.current.createSession() })
    expect(api.newSession).toHaveBeenCalledTimes(1)
  })

  it('createSessionInWorktree applies the pref to the new session too', async () => {
    window.localStorage.setItem(PREF_KEY, 'true')
    vi.mocked(api.switchSession).mockResolvedValue([])
    const { result } = renderHook(() => useApp(), { wrapper })
    await act(async () => { await result.current.createSessionInWorktree() })
    expect(api.setSessionMemoryBypass).toHaveBeenCalledWith('session-new-1', true)
  })
})
