// W10 audit §6-C — AppContext wiring of the cross-window toast dedup.
//
// The three broadcast-event toasts (`session:auto-archived`,
// `session:auto-unarchived`, `model-override-fallback`) must surface once
// across windows, not once per window. The unit-level channel semantics
// (first-window-wins, per-key rule, TTL) live in `toastDedupe.test.ts`;
// THIS file pins the AppContext call sites to the `maybeShowToast` wrapper
// — a handler reverted to a bare `toast.*` fails here, where two windows'
// deliveries of the same backend event can be simulated end to end.
//
// Harness mirrors AppContextSubagentWindow.test.tsx: a real AppProvider
// whose only replacement is a capturing @tauri-apps/api/event fake. Each
// `flush` stands for ONE window's event handler receiving the global
// broadcast; the BroadcastChannel stub (same-process registry, see
// toastDedupe.test.ts) carries the cross-window suppression leg exactly as
// in production.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { renderHook, act, waitFor } from '@testing-library/react'
import { AppProvider, useApp } from '@/context/AppContext'
import { EVENT_NAMES } from '@/types'
import { toast } from 'sonner'
import { TOAST_DEDUP_TTL_MS, resetToastDedupeForTest } from '@/lib/toastDedupe'
import * as api from '@/lib/tauri-api'

vi.mock('sonner', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
    warning: vi.fn(),
    info: vi.fn(),
    message: vi.fn(),
    loading: vi.fn(),
    custom: vi.fn(),
  },
}))

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

/**
 * BroadcastChannel double with real same-process semantics (shared registry
 * per channel name; postMessage delivers to every OTHER live instance) —
 * the dual-window harness from toastDedupe.test.ts.
 */
function makeBroadcastChannelMock() {
  const registry = new Map<string, Set<MockChannel>>()
  class MockChannel {
    name: string
    onmessage: ((ev: { data: unknown }) => void) | null = null
    closed = false
    constructor(name: string) {
      this.name = name
      let set = registry.get(name)
      if (!set) {
        set = new Set()
        registry.set(name, set)
      }
      set.add(this)
    }
    postMessage(data: unknown) {
      for (const peer of registry.get(this.name) ?? []) {
        if (peer !== this && !peer.closed) peer.onmessage?.({ data })
      }
    }
    close() {
      this.closed = true
      registry.get(this.name)?.delete(this)
    }
  }
  return { MockChannel, registry }
}

function wrapper({ children }: { children: React.ReactNode }) {
  return <AppProvider>{children}</AppProvider>
}

async function mountProvider() {
  const utils = renderHook(() => useApp(), { wrapper })
  await waitFor(() => expect(utils.result.current.loading).toBe(false))
  // Wait until the three toast-bearing handlers are registered before any
  // flush — an event fired earlier is simply dropped, which would fake a
  // "suppressed" pass.
  await waitFor(() => {
    expect((captured[EVENT_NAMES.SESSION_AUTO_ARCHIVED] ?? []).length).toBeGreaterThan(0)
    expect((captured[EVENT_NAMES.SESSION_AUTO_UNARCHIVED] ?? []).length).toBeGreaterThan(0)
    expect((captured[EVENT_NAMES.MODEL_OVERRIDE_FALLBACK] ?? []).length).toBeGreaterThan(0)
  })
  // Boot itself must stay toastless for the per-test counts below to mean
  // "deliveries of the flushed event".
  vi.mocked(toast.success).mockClear()
  vi.mocked(toast.warning).mockClear()
  return utils
}

beforeEach(() => {
  vi.clearAllMocks()
  resetToastDedupeForTest()
  localStorage.clear()
  for (const key of Object.keys(captured)) delete captured[key]
  window.history.replaceState(null, '', '/')
  vi.mocked(api.listSessions).mockResolvedValue([])
  vi.mocked(api.switchSession).mockResolvedValue([])
  vi.spyOn(console, 'warn').mockImplementation(() => undefined)
})

afterEach(() => {
  // A stubbed/removed BroadcastChannel or leaked fake timers must never
  // bleed into the next test (the provider boot awaits real timers).
  vi.unstubAllGlobals()
  vi.useRealTimers()
})

describe('W10 §6-C — AppContext broadcast toasts dedupe across windows', () => {
  it('the same auto-archived broadcast delivered to two windows toasts once', async () => {
    const { MockChannel } = makeBroadcastChannelMock()
    vi.stubGlobal('BroadcastChannel', MockChannel)
    await mountProvider()

    const payload = { session_id: 'aaaa1111-0000-4000-8000-00000000aa01', title: 'Research' }
    act(() => {
      flush(EVENT_NAMES.SESSION_AUTO_ARCHIVED, payload) // window A's handler
      flush(EVENT_NAMES.SESSION_AUTO_ARCHIVED, payload) // window B's handler
    })
    expect(toast.success).toHaveBeenCalledTimes(1)
    // The toast names the archived session (interpolation position is
    // locale-dependent — anchor on the title, not the full string).
    expect(vi.mocked(toast.success).mock.calls[0]?.[0]).toEqual(expect.stringContaining('Research'))
  })

  it('different sessions auto-archived in the same sweep each get their toast', async () => {
    const { MockChannel } = makeBroadcastChannelMock()
    vi.stubGlobal('BroadcastChannel', MockChannel)
    await mountProvider()

    act(() => {
      flush(EVENT_NAMES.SESSION_AUTO_ARCHIVED, {
        session_id: 'aaaa1111-0000-4000-8000-00000000aa02',
      })
      flush(EVENT_NAMES.SESSION_AUTO_ARCHIVED, {
        session_id: 'aaaa1111-0000-4000-8000-00000000aa03',
      })
    })
    expect(toast.success).toHaveBeenCalledTimes(2)
  })

  it('the same auto-unarchived broadcast delivered to two windows toasts once', async () => {
    const { MockChannel } = makeBroadcastChannelMock()
    vi.stubGlobal('BroadcastChannel', MockChannel)
    await mountProvider()

    const payload = { session_id: 'bbbb2222-0000-4000-8000-00000000bb01' }
    act(() => {
      flush(EVENT_NAMES.SESSION_AUTO_UNARCHIVED, payload)
      flush(EVENT_NAMES.SESSION_AUTO_UNARCHIVED, payload)
    })
    expect(toast.success).toHaveBeenCalledTimes(1)
  })

  it('model-override-fallback warns once per (session, provider, model) across windows', async () => {
    const { MockChannel } = makeBroadcastChannelMock()
    vi.stubGlobal('BroadcastChannel', MockChannel)
    await mountProvider()

    const sid = 'cccc3333-0000-4000-8000-00000000cc01'
    act(() => {
      // Both windows receive the same fallback event → one warning.
      flush(EVENT_NAMES.MODEL_OVERRIDE_FALLBACK, { session_id: sid, provider: 'anthropic', model: 'claude-x' })
      flush(EVENT_NAMES.MODEL_OVERRIDE_FALLBACK, { session_id: sid, provider: 'anthropic', model: 'claude-x' })
      // A different model's fallback in the same sweep is a distinct key.
      flush(EVENT_NAMES.MODEL_OVERRIDE_FALLBACK, { session_id: sid, provider: 'anthropic', model: 'claude-y' })
    })
    expect(toast.warning).toHaveBeenCalledTimes(2)
  })

  it('the same broadcast toasts again after the dedup TTL expires', async () => {
    const { MockChannel } = makeBroadcastChannelMock()
    vi.stubGlobal('BroadcastChannel', MockChannel)
    const { result } = await mountProvider()
    expect(result.current.loading).toBe(false)

    // Fake timers only AFTER the boot awaits — waitFor needs real timers.
    vi.useFakeTimers()
    try {
      const payload = { session_id: 'dddd4444-0000-4000-8000-00000000dd01' }
      act(() => { flush(EVENT_NAMES.SESSION_AUTO_ARCHIVED, payload) })
      expect(toast.success).toHaveBeenCalledTimes(1)

      act(() => { vi.advanceTimersByTime(TOAST_DEDUP_TTL_MS + 100) })
      // A later auto-archive of the same session is a NEW event.
      act(() => { flush(EVENT_NAMES.SESSION_AUTO_ARCHIVED, payload) })
      expect(toast.success).toHaveBeenCalledTimes(2)
    } finally {
      vi.useRealTimers()
    }
  })

  it('without BroadcastChannel each delivery keeps its own toast (documented degradation)', async () => {
    vi.stubGlobal('BroadcastChannel', undefined)
    await mountProvider()

    const payload = { session_id: 'eeee5555-0000-4000-8000-00000000ee01' }
    act(() => {
      flush(EVENT_NAMES.SESSION_AUTO_ARCHIVED, payload)
      flush(EVENT_NAMES.SESSION_AUTO_ARCHIVED, payload)
    })
    // Degradation is per-window show, never a silent drop.
    expect(toast.success).toHaveBeenCalledTimes(2)
  })
})
