// Settings R3 T5 (B4) — frontend-composited task chime (R4).
//
// A tiny Web Audio "two-tone" chime played when a task completes, fails, or
// needs the user's attention (approval wait). Deliberately frontend-side so
// it sounds identical on every platform and is unit-testable without any
// audio backend; independent from the OS notification sound.
//
// Playback is gated by the cached desktop-notification prefs
// (`api.getNotificationPrefs()`): master switch + the per-event toggle +
// sound_enabled + a frontend-evaluated DND window (the chime is a renderer
// behavior, so it must evaluate DND here, not trust the backend handler).
// `invalidateNotificationPrefsCache()` is wired to CONFIG_UPDATED
// (key = notifications) in AppContext.

import * as api from '@/lib/tauri-api'

/** Semantic kind of the event that wants a chime. */
export type ChimeKind = 'completed' | 'failed' | 'attention'

// === Pure DND window helper =================================================

/** Parse `"HH:MM"` (24h, lenient short forms) into minutes-of-day, or null. */
function parseHhmmToMinutes(s: string | null | undefined): number | null {
  if (!s) return null
  const m = /^(\d{1,2}):(\d{2})$/.exec(s.trim())
  if (!m) return null
  const hours = Number(m[1])
  const minutes = Number(m[2])
  if (hours > 23 || minutes > 59) return null
  return hours * 60 + minutes
}

/**
 * Is `now` inside the daily quiet window `[start, end)`? Handles the
 * overnight wrap (e.g. 22:00 → 07:00). Pure so tests can pin the clock.
 *
 * - Either bound missing or malformed → false (DND effectively unset).
 * - start === end → false (a freshly-enabled DND with identical bounds must
 *   not suppress everything — mirrors the backend's `is_within_dnd_window`).
 */
export function isWithinDnd(
  now: Date,
  start: string | null | undefined,
  end: string | null | undefined,
): boolean {
  const startMin = parseHhmmToMinutes(start)
  const endMin = parseHhmmToMinutes(end)
  if (startMin === null || endMin === null) return false
  if (startMin === endMin) return false
  const nowMin = now.getHours() * 60 + now.getMinutes()
  if (startMin < endMin) {
    return nowMin >= startMin && nowMin < endMin
  }
  // Overnight wrap (e.g. 22:00 → 07:00).
  return nowMin >= startMin || nowMin < endMin
}

// === Prefs gate =============================================================

/** The slice of `api.NotificationPrefs` the chime gate needs. */
export interface ChimePrefs {
  master_enabled: boolean
  sound_enabled?: boolean
  dnd_enabled: boolean
  dnd_start: string | null
  dnd_end: string | null
  on_completed: boolean
  on_failed: boolean
  /** Backend defaults this ON; old cached payloads may omit it. */
  on_needs_attention?: boolean
}

let cachedPrefs: ChimePrefs | null = null
let cachedPrefsPromise: Promise<ChimePrefs | null> | null = null

/** Drop the cached prefs — wired to CONFIG_UPDATED (key = notifications). */
export function invalidateNotificationPrefsCache(): void {
  cachedPrefs = null
  cachedPrefsPromise = null
}

async function loadPrefs(): Promise<ChimePrefs | null> {
  if (cachedPrefs) return cachedPrefs
  if (!cachedPrefsPromise) {
    cachedPrefsPromise = api
      .getNotificationPrefs()
      .then((p) => {
        cachedPrefs = p as ChimePrefs
        return cachedPrefs
      })
      .catch(() => {
        // Prefs unavailable (bridge hiccup, test env) → stay silent, retry
        // on the next event.
        cachedPrefsPromise = null
        return null
      })
  }
  return cachedPrefsPromise
}

/** Would a chime for `kind` be allowed right now under `prefs`? Pure so the
 *  gate matrix is testable without audio or mocks. */
export function chimeAllowed(prefs: ChimePrefs | null, kind: ChimeKind, now: Date): boolean {
  if (!prefs) return false
  if (!prefs.master_enabled) return false
  if (!prefs.sound_enabled) return false
  const perEvent =
    kind === 'completed'
      ? prefs.on_completed
      : kind === 'failed'
        ? prefs.on_failed
        : (prefs.on_needs_attention ?? true)
  if (!perEvent) return false
  if (prefs.dnd_enabled && isWithinDnd(now, prefs.dnd_start, prefs.dnd_end)) return false
  return true
}

/**
 * Play the task chime for `kind` if the cached prefs allow it. Fire-and-
 * forget: never throws, never rejects — a chime is best-effort by design.
 */
export async function maybePlayTaskChime(kind: ChimeKind): Promise<void> {
  try {
    const prefs = await loadPrefs()
    if (!chimeAllowed(prefs, kind, new Date())) return
    playTaskChime(kind)
  } catch {
    // Swallow everything — audio is strictly optional.
  }
}

// === Synthesis ==============================================================

/** Two-tone pitch pairs (Hz) per kind: completed ascends, failed descends. */
const CHIME_FREQUENCIES: Record<ChimeKind, [number, number]> = {
  completed: [1318.51, 1567.98], // E6 → G6
  failed: [1567.98, 1318.51], // G6 → E6
  attention: [1318.51, 1567.98], // E6 → G6 (longer tail — see CHIME_TONE_MS)
}

const CHIME_TONE_MS: Record<ChimeKind, [number, number]> = {
  completed: [150, 150],
  failed: [150, 150],
  attention: [150, 280],
}

const CHIME_GAP_MS = 80
const CHIME_VOLUME = 0.15

let audioContext: AudioContext | null = null

/** Test hook — drop the lazy AudioContext so a mock can be installed. */
export function resetChimeAudioContextForTest(): void {
  audioContext = null
}

function getAudioContext(): AudioContext | null {
  if (audioContext) return audioContext
  try {
    const Ctx =
      window.AudioContext ??
      (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext
    if (!Ctx) return null
    audioContext = new Ctx()
    return audioContext
  } catch {
    return null
  }
}

/**
 * Play the two-tone chime immediately (no prefs gating — see
 * [`maybePlayTaskChime`] for the gated entry point). Synthesized with sine
 * oscillators and a short attack/release envelope so the tones don't click.
 * Silent no-op when Web Audio is unavailable or throws (jsdom, blocked
 * autoplay, …).
 */
export function playTaskChime(kind: ChimeKind = 'completed'): void {
  try {
    const ctx = getAudioContext()
    if (!ctx) return
    if (ctx.state === 'suspended') {
      // Autoplay policies can start the context suspended; resume is async
      // and best-effort — schedule on it anyway, the browser will honor the
      // schedule once it resumes (or drop it silently).
      void ctx.resume().catch(() => undefined)
    }
    const [f1, f2] = CHIME_FREQUENCIES[kind]
    const [d1, d2] = CHIME_TONE_MS[kind]
    const t0 = ctx.currentTime
    scheduleTone(ctx, f1, t0, d1)
    scheduleTone(ctx, f2, t0 + (d1 + CHIME_GAP_MS) / 1000, d2)
  } catch {
    // Never let audio synthesis break the caller.
  }
}

/** One enveloped sine tone at `freq` starting at `startSec`, `durationMs` long. */
function scheduleTone(ctx: AudioContext, freq: number, startSec: number, durationMs: number): void {
  const osc = ctx.createOscillator()
  const gain = ctx.createGain()
  osc.type = 'sine'
  osc.frequency.value = freq
  const start = startSec
  const end = startSec + durationMs / 1000
  const attack = 0.01
  const release = 0.03
  gain.gain.setValueAtTime(0.0001, start)
  gain.gain.exponentialRampToValueAtTime(CHIME_VOLUME, start + attack)
  gain.gain.setValueAtTime(CHIME_VOLUME, Math.max(start + attack, end - release))
  gain.gain.exponentialRampToValueAtTime(0.0001, end)
  osc.connect(gain)
  gain.connect(ctx.destination)
  osc.start(start)
  osc.stop(end)
}
