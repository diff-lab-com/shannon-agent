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
//
// R3 followups F3 — multi-window dedup: every window's AppContext receives
// the same backend events, so a completed/failed/attention chime used to
// sound once PER WINDOW. The dedup layer below (see "Multi-window dedup")
// keeps it to a single audible chime per event without touching the prefs
// gate above.

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

// === Multi-window dedup (F3) ================================================
//
// One audible chime per event across all windows, via two cooperating rules:
//
//  1. Per-key seen rule — every play is announced on
//     `BroadcastChannel('shannon-chime')` as `{key, ts}` and recorded in the
//     local seen table (this window's own marks plus received records). The
//     same key is not replayed while its record is younger than
//     CHIME_DEDUP_TTL_MS — no matter which window sounded it first.
//  2. All-background fallback, per-key — a window plays any key that has no
//     young record, visible or not. A hidden window playing an unseen key is
//     the all-background fallback (a fully minimized setup still gets its
//     notice). Review I1: the fallback is strictly PER-KEY — a DIFFERENT
//     key's recent record must never swallow an event (the old
//     "hidden + any record → silent" rule ate the failure chime that came 1s
//     after the completion chime in an all-background setup).
//
// Per-play ordering: check seen → mark self → play → broadcast. Marking
// before playing makes same-window duplicate deliveries (double listener,
// re-fired event) single-shot; broadcasting after means an announcement never
// precedes an actual sound. Two windows deciding in the same tick — before
// either broadcast has arrived — can both play; that millisecond-scale race
// is accepted. Without BroadcastChannel (SSR, test envs) the layer degrades
// to the pre-F3 direct play via try/catch.

const CHIME_DEDUP_CHANNEL = 'shannon-chime'

/** How long a play record suppresses its key (and the hidden-window fallback). */
export const CHIME_DEDUP_TTL_MS = 2000

/**
 * Keys recently played anywhere we know of — this window's own marks plus
 * records received from other windows. Value = wall-clock ms of the play.
 */
const seenChimePlays = new Map<string, number>()
let chimeChannel: BroadcastChannel | null = null
let chimeChannelUnavailable = false

function getChimeChannel(): BroadcastChannel | null {
  if (chimeChannel) return chimeChannel
  if (chimeChannelUnavailable) return null
  try {
    if (typeof BroadcastChannel === 'undefined') throw new Error('BroadcastChannel unavailable')
    chimeChannel = new BroadcastChannel(CHIME_DEDUP_CHANNEL)
    chimeChannel.onmessage = (ev: MessageEvent) => {
      try {
        const record = ev.data as { key?: unknown; ts?: unknown } | null
        if (!record || typeof record.key !== 'string') return
        seenChimePlays.set(record.key, typeof record.ts === 'number' ? record.ts : Date.now())
      } catch {
        // A malformed record from a peer is never worth throwing over.
      }
    }
    return chimeChannel
  } catch {
    // SSR / environments without BroadcastChannel: remember the failure so we
    // don't retry on every event; callers degrade to direct play.
    chimeChannelUnavailable = true
    return null
  }
}

/** Test hook — drop the seen table and the channel so a test starts clean. */
export function resetChimeDedupForTest(): void {
  seenChimePlays.clear()
  try {
    chimeChannel?.close()
  } catch {
    // Already closed, or a test double without close().
  }
  chimeChannel = null
  chimeChannelUnavailable = false
}

function pruneExpiredChimePlays(now: number): void {
  for (const [key, ts] of seenChimePlays) {
    if (now - ts > CHIME_DEDUP_TTL_MS) seenChimePlays.delete(key)
  }
}

/**
 * Reserve the right to play `key` in THIS window. False means the dedup layer
 * suppresses it: the same key was already played within the TTL (by this
 * window or a received broadcast). Per-key (review I1): suppression never
 * looks at OTHER keys, and window visibility no longer gates playback — a
 * hidden window plays any key nothing has recorded recently (the
 * all-background fallback), which is what keeps a fully minimized setup
 * notified. No BroadcastChannel → always true (pre-F3 direct play).
 */
function tryClaimChime(key: string): boolean {
  if (!getChimeChannel()) return true
  const now = Date.now()
  pruneExpiredChimePlays(now)
  // ① Seen check — the SAME key already sounded within the TTL → stay
  //    silent. Nothing else suppresses: a different key's record (recent
  //    completion vs. this failure, say) is irrelevant to this event.
  if (seenChimePlays.has(key)) return false
  // ② Mark self BEFORE playing so a same-tick duplicate delivery in this
  //    window cannot double-play.
  seenChimePlays.set(key, now)
  return true
}

/** ③ Announce the play to the other windows — strictly after the local play. */
function broadcastChimePlayed(key: string): void {
  try {
    chimeChannel?.postMessage({ key, ts: Date.now() })
  } catch {
    // Announcing is best-effort; the local chime already happened.
  }
}

/**
 * Dedup key for one chime event, derived from the stable payload identifiers
 * all windows see (event name + session id / query id / request id …). Every
 * window derives the SAME key for the same backend event — that is what makes
 * the seen-table check line up across windows. Empty/missing parts are
 * dropped, so legacy payloads without ids still produce a stable key.
 */
export function chimeKey(event: string, ...ids: Array<string | null | undefined>): string {
  return [event, ...ids].filter((part) => typeof part === 'string' && part !== '').join(':')
}

/**
 * Play the task chime for `kind` if the cached prefs allow it AND the
 * multi-window dedup layer lets this window sound it (see the F3 block
 * above). `key` should be derived from stable payload ids via [`chimeKey`];
 * without one the call is treated as always-unique and keeps the pre-dedup
 * always-play behavior. Fire-and-forget: never throws, never rejects — a
 * chime is best-effort by design.
 */
export async function maybePlayTaskChime(kind: ChimeKind, key?: string): Promise<void> {
  try {
    const prefs = await loadPrefs()
    if (!chimeAllowed(prefs, kind, new Date())) return
    // Keyless callers: unique per-call key (same-ms calls included), so every
    // gate-open call plays — the pre-F3 behavior, in this window and
    // elsewhere (per-key suppression can never match a unique key).
    const dedupKey = key ?? `${kind}:#${++keylessChimeSeq}`
    if (!tryClaimChime(dedupKey)) return
    playTaskChime(kind)
    broadcastChimePlayed(dedupKey)
  } catch {
    // Swallow everything — audio is strictly optional.
  }
}

/** Sequence for keyless calls — same-millisecond calls must not share a key. */
let keylessChimeSeq = 0

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
