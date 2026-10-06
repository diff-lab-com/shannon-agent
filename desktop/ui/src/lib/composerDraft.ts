// B1 §4-11 per-session composer draft storage. Extracted verbatim from
// pages/Chat.tsx so non-page code can clear a session's draft without
// importing the chat surface (AppContext cannot reach the page module —
// the dependency would invert and drag the whole chat bundle into the
// context layer; B1-3's deleteSessionAction is the first such caller).

const DRAFT_KEY_PREFIX = 'shannon.draft.'
const DRAFT_MAX_BYTES = 64 * 1024

export function draftKey(sessionId: string): string {
  return `${DRAFT_KEY_PREFIX}${sessionId}`
}

export function readDraft(sessionId: string): { text: string; attachments: string[] } | null {
  try {
    const raw = localStorage.getItem(draftKey(sessionId))
    if (!raw) return null
    const parsed = JSON.parse(raw) as { text?: unknown; attachments?: unknown }
    if (typeof parsed.text !== 'string' || !Array.isArray(parsed.attachments)) return null
    return {
      text: parsed.text,
      attachments: parsed.attachments.filter((a): a is string => typeof a === 'string'),
    }
  } catch { return null }
}

// B1-3-RESIDUE: ids whose draft key must never be (re)written. Deleting the
// OPEN session clears `shannon.draft.<id>` and then flips the pointer to
// null, and Chat's draft switch-flush (plus the 300ms debounce straddling
// the delete) can still persist the composer text under the deleted id —
// resurrecting the just-cleared key as stale residue across restarts. The
// guard lives at this single write choke point (not in Chat.tsx) so both
// straggler paths are covered; deleteSessionAction tombstones before
// clearing. Session ids are UUIDs and never reused, so the set only grows
// with deletions per app run — bounded enough to ignore.
const tombstonedDraftIds = new Set<string>()

export function tombstoneDraft(sessionId: string): void {
  tombstonedDraftIds.add(sessionId)
}

export function writeDraft(sessionId: string, text: string, attachments: string[]): 'saved' | 'oversize' | 'failed' {
  // A tombstoned (deleted) session's draft is dropped, not persisted. Not
  // 'oversize' — the caller toasts on that; a refused residue write is silent.
  if (tombstonedDraftIds.has(sessionId)) return 'failed'
  try {
    const payload = JSON.stringify({ text, attachments, updatedAt: Date.now() })
    // Size cap: a runaway draft must not crowd the quota for the dock's
    // persisted keys. Oversized drafts simply stay in-memory — A-21 fix:
    // the skip used to be silent; the caller now warns (console + a
    // one-shot toast) instead of letting a reload eat the text unnoticed.
    if (payload.length > DRAFT_MAX_BYTES) return 'oversize'
    localStorage.setItem(draftKey(sessionId), payload)
    return 'saved'
  } catch { return 'failed' /* quota / private mode — drafts are best-effort */ }
}

export function clearDraft(sessionId: string): void {
  try { localStorage.removeItem(draftKey(sessionId)) } catch { /* noop */ }
}

/** Persistence cap in KB, for callers that warn about oversized drafts. */
export const DRAFT_MAX_KB = Math.round(DRAFT_MAX_BYTES / 1024)
