/**
 * §J2 pagination as the FAKE engine implements it (dev-only — the only
 * consumer is `dev-standalone.ts`, which production never imports). Extracted
 * as a pure function so the anchor semantics are unit-testable without
 * booting the standalone host.
 *
 * Semantics per the cross-repo spec §J2 (the engine side owns slicing; the
 * gateway proxies verbatim):
 *  - `limit` defaults to 50 and clamps to 1 below that (<1 → 1);
 *  - `before` is an ISO-8601 anchor: the page boundary sits at the first
 *    transcript entry with `ts >= anchor` (i.e. the first occurrence of that
 *    ts, or where it would slot), pulled back to the head of its same-ts run
 *    so a page never splits a recordTurn (user+assistant share one ts);
 *  - the page is the NEWEST `limit` entries strictly older than the boundary,
 *    still time-ascending;
 *  - `hasMore` reports whether anything older remains past the page.
 */

/** One transcript entry as the fake engine stores it. */
export interface FakeHistoryMessage {
  role: string;
  content: string;
  /** Epoch ms when the message was recorded; absent sorts as epoch 0. */
  ts?: unknown;
}

export function fakeHistoryPage(
  messages: readonly FakeHistoryMessage[],
  before: string | undefined,
  limit: number | undefined,
): { page: FakeHistoryMessage[]; hasMore: boolean } {
  const size =
    typeof limit === "number" && Number.isFinite(limit) ? Math.max(1, Math.floor(limit)) : 50;
  const msgTs = (m: FakeHistoryMessage): number => (typeof m.ts === "number" ? m.ts : 0);
  let end = messages.length;
  const beforeMs = typeof before === "string" ? Date.parse(before) : NaN;
  if (Number.isFinite(beforeMs)) {
    const i = messages.findIndex((m) => msgTs(m) >= beforeMs);
    if (i === -1) {
      // Anchor newer than everything → every entry is strictly older.
      end = messages.length;
    } else {
      // Group integrity: never split a same-ts run at the boundary.
      let head = i;
      while (head > 0) {
        const prev = messages[head - 1];
        const cur = messages[head];
        if (!prev || !cur || msgTs(prev) !== msgTs(cur)) break;
        head -= 1;
      }
      end = head;
    }
  }
  return {
    page: messages.slice(Math.max(0, end - size), end),
    hasMore: end > size,
  };
}
