// W10 audit §6-C — cross-window toast dedup.
//
// Every desktop window's AppContext receives the same broadcast backend
// events, so the `session:auto-archived` / `session:auto-unarchived` /
// `model-override-fallback` toasts used to pop up once PER WINDOW. This
// module ports the chime's F3 pattern (`lib/notificationChime.ts`:
// stable key + first window wins) to the toast layer so the same backend
// event surfaces exactly one toast, in whichever window claimed it first.
//
// Same cooperating rules as the chime:
//
//  1. Per-key seen rule — every show is announced on
//     `BroadcastChannel('shannon-toast')` as `{key, ts}` and recorded in the
//     local seen table (this window's own marks plus received records). The
//     same key is not re-shown while its record is younger than
//     TOAST_DEDUP_TTL_MS — no matter which window showed it first.
//  2. Strictly per-key — a DIFFERENT key's recent record never swallows an
//     event (a `model-override-fallback` toast 1s after an auto-archive
//     toast must still come through).
//
// Per-show ordering mirrors the chime: check seen → mark self → show →
// broadcast. Marking before showing makes same-window duplicate deliveries
// single-shot; broadcasting after means an announcement never precedes an
// actual toast. Two windows deciding in the same tick — before either
// broadcast has arrived — can both show; that millisecond-scale race is
// accepted (same as the chime). Without BroadcastChannel (SSR, test envs)
// the layer degrades to the pre-§6-C direct show via try/catch.

/** How long a toast record suppresses its key (across all windows). */
export const TOAST_DEDUP_TTL_MS = 2000

const TOAST_DEDUP_CHANNEL = 'shannon-toast'

/**
 * Keys recently shown anywhere we know of — this window's own marks plus
 * records received from other windows. Value = wall-clock ms of the show.
 */
const seenToastShows = new Map<string, number>()
let toastChannel: BroadcastChannel | null = null
let toastChannelUnavailable = false

function getToastChannel(): BroadcastChannel | null {
  if (toastChannel) return toastChannel
  if (toastChannelUnavailable) return null
  try {
    if (typeof BroadcastChannel === 'undefined') throw new Error('BroadcastChannel unavailable')
    toastChannel = new BroadcastChannel(TOAST_DEDUP_CHANNEL)
    toastChannel.onmessage = (ev: MessageEvent) => {
      try {
        const record = ev.data as { key?: unknown; ts?: unknown } | null
        if (!record || typeof record.key !== 'string') return
        seenToastShows.set(record.key, typeof record.ts === 'number' ? record.ts : Date.now())
      } catch {
        // A malformed record from a peer is never worth throwing over.
      }
    }
    return toastChannel
  } catch {
    // SSR / environments without BroadcastChannel: remember the failure so we
    // don't retry on every event; callers degrade to a direct show.
    toastChannelUnavailable = true
    return null
  }
}

/** Test hook — drop the seen table and the channel so a test starts clean. */
export function resetToastDedupeForTest(): void {
  seenToastShows.clear()
  try {
    toastChannel?.close()
  } catch {
    // Already closed, or a test double without close().
  }
  toastChannel = null
  toastChannelUnavailable = false
}

function pruneExpiredToastShows(now: number): void {
  for (const [key, ts] of seenToastShows) {
    if (now - ts > TOAST_DEDUP_TTL_MS) seenToastShows.delete(key)
  }
}

/**
 * Reserve the right to show `key` in THIS window. False means the dedup
 * layer suppresses it: the same key was already shown within the TTL (by
 * this window or a received broadcast). Per-key: suppression never looks at
 * OTHER keys. No BroadcastChannel → always true (pre-§6-C direct show).
 */
function tryClaimToast(key: string): boolean {
  if (!getToastChannel()) return true
  const now = Date.now()
  pruneExpiredToastShows(now)
  // ① Seen check — the SAME key already surfaced within the TTL → stay
  //    quiet. Nothing else suppresses.
  if (seenToastShows.has(key)) return false
  // ② Mark self BEFORE showing so a same-tick duplicate delivery in this
  //    window cannot double-show.
  seenToastShows.set(key, now)
  return true
}

/** ③ Announce the show to the other windows — strictly after the local one. */
function broadcastToastShown(key: string): void {
  try {
    toastChannel?.postMessage({ key, ts: Date.now() })
  } catch {
    // Announcing is best-effort; the local toast already happened.
  }
}

/**
 * Show the toast via `show` iff this window wins the cross-window claim for
 * `key` (first window wins — see the §6-C block above). Without a key the
 * call is treated as always-unique and keeps the pre-dedup always-show
 * behavior. Synchronous and never throws — a toast is best-effort.
 */
export function maybeShowToast(key: string | null | undefined, show: () => void): void {
  try {
    if (!key) {
      show()
      return
    }
    if (!tryClaimToast(key)) return
    show()
    broadcastToastShown(key)
  } catch {
    // Swallow everything — a toast must never break its event handler.
  }
}
