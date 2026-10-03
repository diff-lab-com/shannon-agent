import { describe, expect, it } from "vitest";

import { fakeHistoryPage, type FakeHistoryMessage } from "../fakeEngineHistory.js";

/** Five recordTurn pairs (user+assistant share one ts), time-ascending. */
function seedTranscript(): FakeHistoryMessage[] {
  const turns = [1000, 2000, 3000, 4000, 5000].map((ts, i) => [
    { role: "user", content: `u${i}`, ts },
    { role: "assistant", content: `a${i}`, ts },
  ]);
  return turns.flat();
}

const iso = (ms: number): string => new Date(ms).toISOString();

describe("FAKE_ENGINE §J2 history page (fakeEngineHistory)", () => {
  it("default page on a short transcript is the full history, hasMore false", () => {
    const short = seedTranscript().slice(0, 2);
    const { page, hasMore } = fakeHistoryPage(short, undefined, undefined);
    expect(page).toHaveLength(2);
    expect(page.map((m) => m.content)).toEqual(["u0", "a0"]);
    expect(hasMore).toBe(false);
  });

  it("limit slices the NEWEST window (§J2), not the head", () => {
    const short = seedTranscript().slice(0, 2);
    const { page, hasMore } = fakeHistoryPage(short, undefined, 1);
    expect(page.map((m) => m.content)).toEqual(["a0"]);
    expect(hasMore).toBe(true);
  });

  it("limit < 1 clamps to 1", () => {
    const { page, hasMore } = fakeHistoryPage(seedTranscript(), undefined, 0);
    expect(page.map((m) => m.content)).toEqual(["a4"]);
    expect(hasMore).toBe(true);
  });

  it("before anchor returns strictly-older entries, same-ts group not split", () => {
    // Anchor at turn 3's ts (4000): strictly older = turns 0–2 (6 entries);
    // the anchor's own group (turn 3) is excluded whole, never half-in.
    const { page, hasMore } = fakeHistoryPage(seedTranscript(), iso(4000), 50);
    expect(page.map((m) => m.content)).toEqual(["u0", "a0", "u1", "a1", "u2", "a2"]);
    expect(hasMore).toBe(false);

    const windowed = fakeHistoryPage(seedTranscript(), iso(4000), 2);
    expect(windowed.page.map((m) => m.content)).toEqual(["u2", "a2"]);
    expect(windowed.hasMore).toBe(true);
  });

  it("sequential loadEarlier pages tile the transcript without repeats or gaps", () => {
    const all = seedTranscript();
    const collected: FakeHistoryMessage[] = [];
    let before: string | undefined;
    for (let guard = 0; guard < 8; guard += 1) {
      const { page, hasMore } = fakeHistoryPage(all, before, 4);
      collected.unshift(...page);
      const oldest = page[0];
      if (!hasMore || !oldest) break;
      before = iso(Number(oldest.ts));
    }
    expect(collected.map((m) => m.content)).toEqual(all.map((m) => m.content));
  });

  it("an anchor newer than every entry still serves the newest window", () => {
    const { page, hasMore } = fakeHistoryPage(seedTranscript(), iso(999_999), 4);
    expect(page.map((m) => m.content)).toEqual(["u3", "a3", "u4", "a4"]);
    expect(hasMore).toBe(true);
  });

  it("ts-less entries sort as epoch 0 (a real anchor always lands after them)", () => {
    const mixed: FakeHistoryMessage[] = [{ role: "user", content: "old" }, { role: "assistant", content: "new", ts: 5000 }];
    const { page, hasMore } = fakeHistoryPage(mixed, iso(1000), 50);
    expect(page.map((m) => m.content)).toEqual(["old"]);
    expect(hasMore).toBe(false);
  });

  it("an empty roster (unknown session) degrades to an empty page", () => {
    expect(fakeHistoryPage([], undefined, undefined)).toEqual({ page: [], hasMore: false });
    expect(fakeHistoryPage([], iso(1000), 4)).toEqual({ page: [], hasMore: false });
  });
});
