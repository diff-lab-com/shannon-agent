// Settings R3 T5 — notificationChime unit tests.
//
// Covers: the pure DND window helper (incl. the overnight wrap), the chime
// gate matrix (`chimeAllowed`), and the Web Audio synthesis path against a
// fake AudioContext (no throw + oscillator call sequence).

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import * as api from '@/lib/tauri-api'
import {
  isWithinDnd,
  chimeAllowed,
  playTaskChime,
  maybePlayTaskChime,
  invalidateNotificationPrefsCache,
  resetChimeAudioContextForTest,
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
  getNotificationPrefs.mockResolvedValue(allOnPrefs)
})

afterEach(() => {
  vi.unstubAllGlobals()
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
