// A-22 — global composer input history (terminal-style ArrowUp/ArrowDown
// recall in the chat composer).
//
// One ring for the whole app, NOT per session — the user's mental model is
// "what did I just send", independent of which chat produced it. The page
// (Chat.tsx) records an entry when a send is accepted or a queue join is
// accepted; ChatInput walks the ring with the arrow keys.
//
// Persistence mirrors the per-session draft in Chat.tsx (B1 §4-11): plain
// localStorage, every read/write wrapped in try/catch — quota errors,
// private mode and corrupted payloads degrade to "no history", never an
// exception in the send or recall paths.
//
// Order contract: the array ends with the MOST RECENT entry (MRU, oldest
// first). Recording moves an already-present entry to the end; a repeat of
// the current last entry is a no-op — re-sending the same prompt never
// drowns the ring in duplicates.

export const INPUT_HISTORY_KEY = 'shannon.inputHistory'

export const INPUT_HISTORY_LIMIT = 50

/** Read the ring, oldest first. Missing/corrupted data degrades to []. */
export function loadInputHistory(): string[] {
  try {
    const raw = localStorage.getItem(INPUT_HISTORY_KEY)
    if (!raw) return []
    const parsed: unknown = JSON.parse(raw)
    if (!Array.isArray(parsed)) return []
    return parsed
      .filter((e): e is string => typeof e === 'string')
      .slice(-INPUT_HISTORY_LIMIT)
  } catch {
    // Quota-exceeded read, private mode, corrupted JSON — best-effort,
    // same contract as the composer draft.
    return []
  }
}

/**
 * Record a sent prompt. Returns the resulting ring (oldest first).
 *
 *  - empty entry → ignored
 *  - identical to the current last entry → ring unchanged (no duplicate)
 *  - already present earlier → moved to the end (MRU)
 *  - past {@link INPUT_HISTORY_LIMIT} → the oldest entry is dropped
 *
 * The localStorage write is best-effort: a failed write still returns the
 * in-memory ring so the current session can recall what it sent.
 */
export function recordInputHistory(entry: string): string[] {
  const history = loadInputHistory()
  if (!entry || history[history.length - 1] === entry) return history
  const next = history.filter(h => h !== entry)
  next.push(entry)
  while (next.length > INPUT_HISTORY_LIMIT) next.shift()
  try {
    localStorage.setItem(INPUT_HISTORY_KEY, JSON.stringify(next))
  } catch { /* best-effort, same as the composer draft */ }
  return next
}
