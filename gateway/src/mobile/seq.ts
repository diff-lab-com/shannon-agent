/**
 * Push-notification sequence counter (WP-15 T4 — mobile live-sync contract).
 *
 * The phone's `LiveSync` keeps a `lastSeq` cursor over **push notifications**
 * (`shannon/event` / `shannon/task.progress` params top-level `seq`) and, on
 * reconnect, reconciles via `shannon/snapshot` or `shannon/resume(sinceSeq)`.
 * Contract (shannon-mobile live_sync.dart / mock_server.dart):
 *
 *  - one global monotonically increasing counter, starting at 1;
 *  - every pushed notification's `params` carries the next `seq` (top level);
 *  - notifications WITHOUT a seq are simply invisible to the cursor;
 *  - after a gateway restart the counter resets — the phone's stale cursor
 *    then exceeds our seq, which the resume path rejects with `gapTooLarge`
 *    and the phone falls back to a snapshot. Safe by construction.
 */

export interface SeqCounter {
  next(): number;
  current(): number;
}

export function createSeqCounter(start = 0): SeqCounter {
  let n = start;
  return {
    next: () => {
      n += 1;
      return n;
    },
    current: () => n,
  };
}

/** Process-wide push counter shared by every push fan-out path. */
export const sharedPushSeq: SeqCounter = createSeqCounter();

/** Default retained-gap window: resume is rejected beyond this many events. */
export const GAP_WINDOW = 1000;
