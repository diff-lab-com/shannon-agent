// W10 audit §6-C — toastDedupe unit tests.
//
// Covers the cross-window toast dedup layer (port of the chime F3 pattern,
// `lib/notificationChime.ts`): per-key seen rule, first-window-wins across
// two simulated windows (shared BroadcastChannel registry), strictly-per-key
// suppression, TTL expiry, and the no-BroadcastChannel degradation.

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import {
  maybeShowToast,
  TOAST_DEDUP_TTL_MS,
  resetToastDedupeForTest,
} from '@/lib/toastDedupe'
import { chimeKey } from '@/lib/notificationChime'

/**
 * BroadcastChannel double with real same-process semantics: instances of the
 * same channel name live in a shared registry and postMessage delivers to
 * every OTHER live instance — so a test can open a "second window" channel
 * and the module's channel receives its records (and vice versa). This is
 * the dual-window harness: both windows' handlers run in one process, the
 * channel does the cross-window leg exactly as in production.
 */
function makeBroadcastChannelMock() {
  const registry = new Map<string, Set<MockChannel>>()
  class MockChannel {
    name: string
    onmessage: ((ev: { data: unknown }) => void) | null = null
    posted: Array<unknown> = []
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
      this.posted.push(data)
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

beforeEach(() => {
  resetToastDedupeForTest()
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('multi-window toast dedup (§6-C)', () => {
  it('keys derive from the stable payload ids via chimeKey', () => {
    expect(chimeKey('toast:auto-archived', 'sess-1')).toBe('toast:auto-archived:sess-1')
    expect(chimeKey('toast:override-fallback', 'sess-1', 'anthropic', 'claude-x')).toBe(
      'toast:override-fallback:sess-1:anthropic:claude-x',
    )
  })

  it('shows the same key only once across two event deliveries', () => {
    const { MockChannel } = makeBroadcastChannelMock()
    vi.stubGlobal('BroadcastChannel', MockChannel)
    const show = vi.fn()

    maybeShowToast('toast:auto-archived:s1', show)
    maybeShowToast('toast:auto-archived:s1', show)
    expect(show).toHaveBeenCalledTimes(1)
  })

  it('dual-window: the second window stays quiet for a key the first window showed', () => {
    const { MockChannel, registry } = makeBroadcastChannelMock()
    vi.stubGlobal('BroadcastChannel', MockChannel)
    const showWindowA = vi.fn()
    const showWindowB = vi.fn()

    // Window A receives the backend event first and shows the toast.
    maybeShowToast('toast:auto-unarchived:s1', showWindowA)
    expect(showWindowA).toHaveBeenCalledTimes(1)
    // Its announcement is live on the shared channel.
    expect(registry.get('shannon-toast')?.size).toBe(1)
    const [channelA] = [...(registry.get('shannon-toast') ?? [])]
    expect(channelA?.posted).toEqual([{ key: 'toast:auto-unarchived:s1', ts: expect.any(Number) }])

    // Window B's handler for the SAME backend event: suppressed.
    maybeShowToast('toast:auto-unarchived:s1', showWindowB)
    expect(showWindowB).not.toHaveBeenCalled()
  })

  it('suppresses a key announced by another window via the received record', () => {
    const { MockChannel } = makeBroadcastChannelMock()
    vi.stubGlobal('BroadcastChannel', MockChannel)
    const show = vi.fn()

    // First call opens the module's channel ('shannon-toast'); a sibling
    // window's channel then shares the registry with it.
    maybeShowToast('warmup', show)
    const sibling = new MockChannel('shannon-toast')
    // The other window showed this backend event's toast first:
    sibling.postMessage({ key: 'toast:auto-archived:s1', ts: Date.now() })

    // This window's handler for the same event: suppressed.
    maybeShowToast('toast:auto-archived:s1', show)
    expect(show).toHaveBeenCalledTimes(1) // only the warmup
  })

  it('different keys never swallow each other (per-key rule)', () => {
    const { MockChannel } = makeBroadcastChannelMock()
    vi.stubGlobal('BroadcastChannel', MockChannel)
    const show = vi.fn()

    maybeShowToast('toast:auto-archived:s1', show)
    // A different session's auto-archive toast moments later still shows.
    maybeShowToast('toast:auto-archived:s2', show)
    // And a different event kind entirely (the override-fallback 1s later).
    maybeShowToast('toast:override-fallback:s1:anthropic:claude-x', show)
    expect(show).toHaveBeenCalledTimes(3)
  })

  it('shows again after the TTL expires', () => {
    vi.useFakeTimers()
    try {
      const { MockChannel } = makeBroadcastChannelMock()
      vi.stubGlobal('BroadcastChannel', MockChannel)
      const show = vi.fn()

      maybeShowToast('toast:auto-archived:s1', show)
      expect(show).toHaveBeenCalledTimes(1)

      vi.advanceTimersByTime(TOAST_DEDUP_TTL_MS + 100)
      // The same (session, provider, model) pin going stale again on a
      // later query is a NEW event — it must toast again.
      maybeShowToast('toast:auto-archived:s1', show)
      expect(show).toHaveBeenCalledTimes(2)
    } finally {
      vi.useRealTimers()
    }
  })

  it('degrades to always-show without BroadcastChannel', () => {
    vi.stubGlobal('BroadcastChannel', undefined)
    const show = vi.fn()

    maybeShowToast('toast:auto-archived:s1', show)
    maybeShowToast('toast:auto-archived:s1', show)
    expect(show).toHaveBeenCalledTimes(2)
  })

  it('a keyless call keeps the always-show behavior and never claims', () => {
    const { MockChannel } = makeBroadcastChannelMock()
    vi.stubGlobal('BroadcastChannel', MockChannel)
    const show = vi.fn()

    maybeShowToast(undefined, show)
    maybeShowToast(null, show)
    maybeShowToast('', show)
    expect(show).toHaveBeenCalledTimes(3)
  })

  it('never throws — a failing toast callback is swallowed', () => {
    const { MockChannel } = makeBroadcastChannelMock()
    vi.stubGlobal('BroadcastChannel', MockChannel)

    expect(() =>
      maybeShowToast('toast:auto-archived:s1', () => {
        throw new Error('sonner is down')
      }),
    ).not.toThrow()
  })
})
