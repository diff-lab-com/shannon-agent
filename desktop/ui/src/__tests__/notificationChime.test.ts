// Settings R3 T5 — notificationChime unit tests.
//
// Covers: the pure DND window helper (incl. the overnight wrap), the chime
// gate matrix (`chimeAllowed`), the Web Audio synthesis path against a
// fake AudioContext (no throw + oscillator call sequence), and the F3
// multi-window dedup layer (visibility rule, all-background fallback,
// BroadcastChannel records, TTL expiry) against a faithful channel double.

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import * as api from '@/lib/tauri-api'
import {
  isWithinDnd,
  chimeAllowed,
  playTaskChime,
  maybePlayTaskChime,
  chimeKey,
  CHIME_DEDUP_TTL_MS,
  invalidateNotificationPrefsCache,
  resetChimeAudioContextForTest,
  resetChimeDedupForTest,
  type ChimePrefs,
} from '@/lib/notificationChime'

const getNotificationPrefs = vi.mocked(api.getNotificationPrefs)

/** Minimal AudioContext double recording oscillator construction. */
function makeFakeAudioContext(opts: { state?: string; broken?: boolean } = {}) {
  const constructed: Array<{ type: string; freq: number }> = []
  class FakeGainNode {
    gain = {
      value: 0,
      setValueAtTime: vi.fn(),
      exponentialRampToValueAtTime: vi.fn(),
    }
    connect = vi.fn()
  }
  class FakeOscillatorNode {
    type = ''
    frequency = { value: 0 }
    connect = vi.fn()
    start = vi.fn()
    stop = vi.fn()
  }
  class FakeAudioContext {
    state = opts.state ?? 'running'
    currentTime = 123.5
    destination = { fake: true }
    resumed = 0
    constructor() {
      if (opts.broken) throw new Error('audio backend unavailable')
    }
    resume() {
      this.resumed += 1
      return Promise.resolve()
    }
    createOscillator() {
      const osc = new FakeOscillatorNode()
      return osc
    }
    createGain() {
      return new FakeGainNode()
    }
  }
  return { FakeAudioContext, constructed }
}

/** Attach freq capture by wrapping createOscillator. Returns the oscillator
 *  nodes themselves — `.frequency.value` is assigned by the chime module
 *  right after creation, so assertions read it off the retained node. */
function captureOscillators(Ctx: new () => unknown): Array<{ frequency: { value: number } }> {
  const oscs: Array<{ frequency: { value: number } }> = []
  const proto = (Ctx as { prototype: Record<string, unknown> }).prototype
  const original = proto.createOscillator as () => { frequency: { value: number } }
  proto.createOscillator = function (this: unknown) {
    const osc = original.call(this)
    oscs.push(osc)
    return osc
  }
  return oscs
}

const allOnPrefs: ChimePrefs = {
  master_enabled: true,
  sound_enabled: true,
  dnd_enabled: false,
  dnd_start: null,
  dnd_end: null,
  on_completed: true,
  on_failed: true,
  on_needs_attention: true,
}

beforeEach(() => {
  vi.clearAllMocks()
  resetChimeAudioContextForTest()
  invalidateNotificationPrefsCache()
  resetChimeDedupForTest()
  getNotificationPrefs.mockResolvedValue(allOnPrefs)
})

afterEach(() => {
  vi.unstubAllGlobals()
  // Drop a test's own visibilityState shadow (jsdom's prototype getter —
  // 'prerender' in jsdom — resumes).
  Reflect.deleteProperty(document, 'visibilityState')
})

describe('isWithinDnd', () => {
  const at = (h: number, m = 0) => new Date(2026, 9, 5, h, m)

  it('returns false when either bound is missing or malformed', () => {
    expect(isWithinDnd(at(12), null, null)).toBe(false)
    expect(isWithinDnd(at(12), '22:00', null)).toBe(false)
    expect(isWithinDnd(at(12), null, '07:00')).toBe(false)
    expect(isWithinDnd(at(12), 'bad', '07:00')).toBe(false)
    expect(isWithinDnd(at(12), '24:00', '07:00')).toBe(false)
    expect(isWithinDnd(at(12), '', '')).toBe(false)
  })

  it('treats equal bounds as no window', () => {
    expect(isWithinDnd(at(12), '22:00', '22:00')).toBe(false)
  })

  it('same-day window is [start, end)', () => {
    // 09:00–17:00
    expect(isWithinDnd(at(9), '09:00', '17:00')).toBe(true)
    expect(isWithinDnd(at(12, 30), '09:00', '17:00')).toBe(true)
    expect(isWithinDnd(at(17), '09:00', '17:00')).toBe(false) // exclusive end
    expect(isWithinDnd(at(8, 59), '09:00', '17:00')).toBe(false)
  })

  it('handles the overnight wrap (22:00 → 07:00)', () => {
    expect(isWithinDnd(at(22), '22:00', '07:00')).toBe(true) // at start
    expect(isWithinDnd(at(23, 30), '22:00', '07:00')).toBe(true) // late night
    expect(isWithinDnd(at(0), '22:00', '07:00')).toBe(true) // midnight
    expect(isWithinDnd(at(3), '22:00', '07:00')).toBe(true) // early morning
    expect(isWithinDnd(at(6, 59), '22:00', '07:00')).toBe(true)
    expect(isWithinDnd(at(7), '22:00', '07:00')).toBe(false) // exclusive end
    expect(isWithinDnd(at(12), '22:00', '07:00')).toBe(false) // midday
  })
})

describe('chimeAllowed (pure gate matrix)', () => {
  it('silences everything when the master switch is off', () => {
    const prefs = { ...allOnPrefs, master_enabled: false }
    expect(chimeAllowed(prefs, 'completed', new Date())).toBe(false)
    expect(chimeAllowed(prefs, 'failed', new Date())).toBe(false)
    expect(chimeAllowed(prefs, 'attention', new Date())).toBe(false)
  })

  it('requires the sound opt-in (default off)', () => {
    expect(chimeAllowed({ ...allOnPrefs, sound_enabled: false }, 'completed', new Date())).toBe(false)
    expect(chimeAllowed({ ...allOnPrefs, sound_enabled: undefined }, 'completed', new Date())).toBe(false)
    expect(chimeAllowed({ ...allOnPrefs, sound_enabled: true }, 'completed', new Date())).toBe(true)
  })

  it('routes each kind to its own toggle', () => {
    const prefs = { ...allOnPrefs, on_completed: false, on_failed: false, on_needs_attention: false }
    expect(chimeAllowed(prefs, 'completed', new Date())).toBe(false)
    expect(chimeAllowed(prefs, 'failed', new Date())).toBe(false)
    expect(chimeAllowed(prefs, 'attention', new Date())).toBe(false)
    expect(chimeAllowed({ ...prefs, on_needs_attention: true }, 'attention', new Date())).toBe(true)
    // Old cached payload without on_needs_attention defaults it ON.
    const legacy = { ...prefs, on_needs_attention: undefined }
    expect(chimeAllowed(legacy, 'attention', new Date())).toBe(true)
  })

  it('suppresses inside the DND window only', () => {
    const prefs: ChimePrefs = {
      ...allOnPrefs,
      dnd_enabled: true,
      dnd_start: '22:00',
      dnd_end: '07:00',
    }
    // 23:00 is inside 22:00–07:00 → silent.
    expect(chimeAllowed(prefs, 'completed', new Date(2026, 9, 5, 23, 0))).toBe(false)
    // 12:00 is outside → allowed.
    expect(chimeAllowed(prefs, 'completed', new Date(2026, 9, 5, 12, 0))).toBe(true)
    // DND enabled but no bounds → no window → allowed.
    expect(
      chimeAllowed({ ...prefs, dnd_start: null, dnd_end: null }, 'completed', new Date(2026, 9, 5, 23, 0)),
    ).toBe(true)
    // DND disabled → allowed even inside the bounds.
    expect(
      chimeAllowed({ ...prefs, dnd_enabled: false }, 'completed', new Date(2026, 9, 5, 23, 0)),
    ).toBe(true)
  })

  it('returns false for missing prefs', () => {
    expect(chimeAllowed(null, 'completed', new Date())).toBe(false)
  })
})

describe('playTaskChime (synthesis, mocked AudioContext)', () => {
  it('creates two enveloped oscillators and does not throw', () => {
    const { FakeAudioContext } = makeFakeAudioContext()
    const oscs = captureOscillators(FakeAudioContext)
    vi.stubGlobal('AudioContext', FakeAudioContext)

    expect(() => playTaskChime('completed')).not.toThrow()
    // Two-tone chime: E6 → G6 for completed.
    expect(oscs.map((o) => o.frequency.value)).toEqual([1318.51, 1567.98])
  })

  it('descending pair for failed', () => {
    const { FakeAudioContext } = makeFakeAudioContext()
    const oscs = captureOscillators(FakeAudioContext)
    vi.stubGlobal('AudioContext', FakeAudioContext)

    expect(() => playTaskChime('failed')).not.toThrow()
    expect(oscs.map((o) => o.frequency.value)).toEqual([1567.98, 1318.51])
  })

  it('is a silent no-op without any AudioContext (jsdom default)', () => {
    expect(() => playTaskChime('completed')).not.toThrow()
  })

  it('swallows a broken audio backend (constructor throws)', () => {
    const { FakeAudioContext } = makeFakeAudioContext({ broken: true })
    vi.stubGlobal('AudioContext', FakeAudioContext)
    expect(() => playTaskChime('completed')).not.toThrow()
    expect(() => playTaskChime('attention')).not.toThrow()
  })

  it('swallows oscillators that fail mid-schedule', () => {
    class HalfBrokenContext {
      state = 'running'
      currentTime = 0
      destination = {}
      createOscillator() {
        throw new Error('oscillator unavailable')
      }
      createGain() {
        throw new Error('unreachable')
      }
    }
    vi.stubGlobal('AudioContext', HalfBrokenContext)
    expect(() => playTaskChime('completed')).not.toThrow()
  })

  it('reuses a single lazy AudioContext across plays', () => {
    const { FakeAudioContext } = makeFakeAudioContext()
    const instances: unknown[] = []
    const Wrapped = class extends (FakeAudioContext as unknown as new () => object) {
      constructor() {
        super()
        instances.push(this)
      }
    }
    vi.stubGlobal('AudioContext', Wrapped)
    playTaskChime('completed')
    playTaskChime('failed')
    expect(instances).toHaveLength(1)
  })
})

describe('maybePlayTaskChime (prefs-gated entry point)', () => {
  it('plays when every gate is open', async () => {
    const { FakeAudioContext } = makeFakeAudioContext()
    const oscs = captureOscillators(FakeAudioContext)
    vi.stubGlobal('AudioContext', FakeAudioContext)
    getNotificationPrefs.mockResolvedValue(allOnPrefs)

    await maybePlayTaskChime('completed')
    expect(getNotificationPrefs).toHaveBeenCalledTimes(1)
    expect(oscs.map((o) => o.frequency.value)).toEqual([1318.51, 1567.98])
  })

  it('stays silent when sound is disabled or the master is off', async () => {
    const { FakeAudioContext } = makeFakeAudioContext()
    const oscs = captureOscillators(FakeAudioContext)
    vi.stubGlobal('AudioContext', FakeAudioContext)

    getNotificationPrefs.mockResolvedValue({ ...allOnPrefs, sound_enabled: false })
    await maybePlayTaskChime('completed')
    expect(oscs).toEqual([])

    getNotificationPrefs.mockResolvedValue({ ...allOnPrefs, master_enabled: false })
    await maybePlayTaskChime('completed')
    expect(oscs).toEqual([])
  })

  it('caches the prefs read and re-reads after invalidation', async () => {
    const { FakeAudioContext } = makeFakeAudioContext()
    const oscs = captureOscillators(FakeAudioContext)
    vi.stubGlobal('AudioContext', FakeAudioContext)
    getNotificationPrefs.mockResolvedValue(allOnPrefs)

    // Two events, one prefs read (cached) — each open-gated event chimes.
    await maybePlayTaskChime('completed')
    await maybePlayTaskChime('completed')
    expect(getNotificationPrefs).toHaveBeenCalledTimes(1)
    expect(oscs).toHaveLength(4)

    // CONFIG_UPDATED(key=notifications) → invalidate → next event re-reads,
    // and a freshly-disabled sound goes silent without an app restart.
    invalidateNotificationPrefsCache()
    getNotificationPrefs.mockResolvedValue({ ...allOnPrefs, sound_enabled: false })
    await maybePlayTaskChime('completed')
    expect(getNotificationPrefs).toHaveBeenCalledTimes(2)
    expect(oscs).toHaveLength(4) // no additional chime
  })

  it('survives a failing prefs read (no chime, no throw)', async () => {
    const { FakeAudioContext } = makeFakeAudioContext()
    const oscs = captureOscillators(FakeAudioContext)
    vi.stubGlobal('AudioContext', FakeAudioContext)
    getNotificationPrefs.mockRejectedValue(new Error('bridge down'))

    await expect(maybePlayTaskChime('failed')).resolves.toBeUndefined()
    expect(oscs).toEqual([])
  })
})

// === F3 multi-window dedup ==================================================

/** Own-property visibilityState shadow. jsdom reports 'prerender' (never
 *  'visible'), so dedup tests always pin the state explicitly. afterEach in
 *  this file deletes the own property, restoring jsdom's prototype getter. */
function stubVisibility(state: 'visible' | 'hidden' | 'prerender'): void {
  Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => state })
}

/**
 * BroadcastChannel double with real same-process semantics: instances of the
 * same channel name live in a shared registry and postMessage delivers to
 * every OTHER live instance — so a test can open a "sibling window" channel
 * and the module's channel receives its records (and vice versa).
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

describe('multi-window dedup (F3)', () => {
  /** Audio stub + oscillator capture; each two-tone chime = 2 oscillators. */
  function stubAudio() {
    const { FakeAudioContext } = makeFakeAudioContext()
    const oscs = captureOscillators(FakeAudioContext)
    vi.stubGlobal('AudioContext', FakeAudioContext)
    return oscs
  }

  it('derives stable keys via chimeKey (missing parts dropped)', () => {
    expect(chimeKey('query:completed', 's1', 'q1')).toBe('query:completed:s1:q1')
    expect(chimeKey('permission', null, 'req-1')).toBe('permission:req-1')
    expect(chimeKey('query:failed', undefined, undefined)).toBe('query:failed')
  })

  it('plays the same key only once across two events', async () => {
    const oscs = stubAudio()
    stubVisibility('visible')

    await maybePlayTaskChime('completed', 'query:completed:s1:q1')
    await maybePlayTaskChime('completed', 'query:completed:s1:q1')
    expect(oscs).toHaveLength(2) // one two-tone chime, not two
  })

  it('plays different keys independently', async () => {
    const oscs = stubAudio()
    stubVisibility('visible')

    await maybePlayTaskChime('completed', 'k1')
    await maybePlayTaskChime('failed', 'k2')
    expect(oscs).toHaveLength(4)
  })

  it('suppresses a key announced by another window (received record)', async () => {
    const oscs = stubAudio()
    stubVisibility('visible')
    const { MockChannel, registry } = makeBroadcastChannelMock()
    vi.stubGlobal('BroadcastChannel', MockChannel)

    // First call opens the module's channel ('shannon-chime'); a sibling
    // window's channel then shares the registry with it.
    await maybePlayTaskChime('completed', 'warmup')
    expect([...(registry.get('shannon-chime') ?? [])]).toHaveLength(1)
    const sibling = new MockChannel('shannon-chime')
    sibling.postMessage({ key: 'k1', ts: Date.now() })

    // Visible, gates open, but another window already sounded k1 → silent.
    await maybePlayTaskChime('completed', 'k1')
    expect(oscs).toHaveLength(2) // only the warmup chime
  })

  it('broadcasts {key, ts} after the local play', async () => {
    const oscs = stubAudio()
    stubVisibility('visible')
    const { MockChannel, registry } = makeBroadcastChannelMock()
    vi.stubGlobal('BroadcastChannel', MockChannel)

    await maybePlayTaskChime('completed', 'k1')
    expect(oscs).toHaveLength(2)
    const [mine] = [...(registry.get('shannon-chime') ?? [])]
    expect(mine?.posted).toEqual([{ key: 'k1', ts: expect.any(Number) }])
  })

  it('hidden window plays as the all-background fallback (no records)', async () => {
    const oscs = stubAudio()
    stubVisibility('hidden')
    const { MockChannel } = makeBroadcastChannelMock()
    vi.stubGlobal('BroadcastChannel', MockChannel)

    // Nothing has played anywhere → the fallback must fire so an
    // all-windows-minimized setup still gets its notice.
    await maybePlayTaskChime('completed', 'k1')
    expect(oscs).toHaveLength(2)
  })

  it('hidden window stays silent once any window played within the TTL', async () => {
    const oscs = stubAudio()
    stubVisibility('hidden')
    const { MockChannel } = makeBroadcastChannelMock()
    vi.stubGlobal('BroadcastChannel', MockChannel)

    await maybePlayTaskChime('completed', 'k1') // fallback play (seen table empty)
    // A different key, but a window already played within the TTL and this
    // window is hidden — the focused window owns the sound.
    await maybePlayTaskChime('failed', 'k2')
    expect(oscs).toHaveLength(2)
  })

  it('plays again after the TTL expires', async () => {
    vi.useFakeTimers()
    try {
      const oscs = stubAudio()
      stubVisibility('visible')
      const { MockChannel } = makeBroadcastChannelMock()
      vi.stubGlobal('BroadcastChannel', MockChannel)

      await maybePlayTaskChime('completed', 'k1')
      expect(oscs).toHaveLength(2)

      vi.advanceTimersByTime(CHIME_DEDUP_TTL_MS + 100)
      await maybePlayTaskChime('completed', 'k1')
      expect(oscs).toHaveLength(4)
    } finally {
      vi.useRealTimers()
    }
  })

  it('received records older than the TTL no longer suppress', async () => {
    vi.useFakeTimers()
    try {
      const oscs = stubAudio()
      stubVisibility('visible')
      const { MockChannel } = makeBroadcastChannelMock()
      vi.stubGlobal('BroadcastChannel', MockChannel)

      await maybePlayTaskChime('completed', 'warmup')
      const sibling = new MockChannel('shannon-chime')
      sibling.postMessage({ key: 'k1', ts: Date.now() })
      await maybePlayTaskChime('completed', 'k1')
      expect(oscs).toHaveLength(2)

      vi.advanceTimersByTime(CHIME_DEDUP_TTL_MS + 100)
      await maybePlayTaskChime('completed', 'k1')
      expect(oscs).toHaveLength(4)
    } finally {
      vi.useRealTimers()
    }
  })

  it('degrades to direct play without BroadcastChannel (even hidden)', async () => {
    const oscs = stubAudio()
    stubVisibility('hidden')
    vi.stubGlobal('BroadcastChannel', undefined)

    // No cross-window channel → pre-F3 behavior: every gates-open event plays.
    await maybePlayTaskChime('completed', 'k1')
    await maybePlayTaskChime('completed', 'k1')
    expect(oscs).toHaveLength(4)
  })

  it('keyless calls keep the pre-F3 always-play behavior', async () => {
    const oscs = stubAudio()
    stubVisibility('visible')
    const { MockChannel } = makeBroadcastChannelMock()
    vi.stubGlobal('BroadcastChannel', MockChannel)

    // Unique per-call keys → no same-key suppression for legacy callers.
    await maybePlayTaskChime('completed')
    await maybePlayTaskChime('completed')
    expect(oscs).toHaveLength(4)
  })

  it('a hidden keyless call after another play stays silent (fallback rule)', async () => {
    const oscs = stubAudio()
    stubVisibility('hidden')
    const { MockChannel } = makeBroadcastChannelMock()
    vi.stubGlobal('BroadcastChannel', MockChannel)

    await maybePlayTaskChime('completed', 'k1')
    await maybePlayTaskChime('failed') // hidden + records exist → suppressed
    expect(oscs).toHaveLength(2)
  })
})
