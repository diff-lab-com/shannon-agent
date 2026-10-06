/**
 * §O4 live-sync replay buffer (cross-repo spec §O4 / m1-roadmap §4 收敛点) —
 * the gateway's per-device ring of recently pushed notifications, so
 * `shannon/resume(sinceSeq)` can answer `replayed` with the events the phone
 * missed while its sockets were down instead of always empty (the phone then
 * converges via re-fetch alone).
 *
 * Records happen at the two seq-stamped push points — `MobileDispatchHub#
 * pushEvent` (task stream, broadcasts) and the direct-query stream loop in
 * `dispatch.ts` — regardless of socket state: an offline device's push is
 * exactly what the buffer exists to replay later.
 *
 * Replay is per-device by construction: `seq` is one global counter, but
 * each device's stream is its own sparse subsequence of it (interleaved with
 * other devices'), so entries never cross devices and a device's ring is a
 * contiguous run of its own stream.
 *
 * Hole semantics: `replay()` reports `complete: false` when `sinceSeq`
 * predates the device's oldest buffered entry — the buffered stream has a
 * gap, and the resume path must answer GAP_TOO_LARGE so the phone
 * re-snapshots (same contract as the global GAP_WINDOW check, which still
 * runs first). A device with NO buffered entries is trivially complete:
 * `replayed: []` is exactly the pre-buffer answer, and the phone's domain
 * providers converge by re-fetch as before. After a gateway restart both
 * the counter and the buffer reset — a stale cursor finds no entries newer
 * than itself and gets the safe empty replay (seq.ts's "safe by
 * construction" note still holds).
 */

import { GAP_WINDOW } from "./seq.js";
import type { ShannonEvent } from "./protocol.js";

/** One buffered push: seq + the event as it rode the notification. */
export interface ReplayEntry {
  seq: number;
  event: ShannonEvent;
}

export interface PushReplayBufferOptions {
  /**
   * Ring size per device. Default GAP_WINDOW — aligned with the global gap
   * check so any gap it accepts is fully covered by the buffer.
   */
  capacityPerDevice?: number;
}

export interface ReplayResult {
  /** The device's entries with `seq > sinceSeq`, time-ascending. */
  entries: ReplayEntry[];
  /**
   * False when `sinceSeq` predates the device's oldest buffered entry (the
   * buffered stream has a hole); `entries` is then empty and the caller
   * answers GAP_TOO_LARGE.
   */
  complete: boolean;
}

export class PushReplayBuffer {
  private readonly rings = new Map<string, ReplayEntry[]>();
  /**
   * Devices whose ring has evicted at least one entry. Until eviction a
   * device's ring holds its ENTIRE stream, so any cursor is coverable; after
   * eviction, a cursor below the oldest retained entry is a real hole.
   */
  private readonly evicted = new Set<string>();
  private readonly capacity: number;

  constructor(opts: PushReplayBufferOptions = {}) {
    this.capacity = opts.capacityPerDevice ?? GAP_WINDOW;
  }

  /** Record one pushed notification (at seq-stamp time, online or not). */
  record(deviceId: string, seq: number, event: ShannonEvent): void {
    let ring = this.rings.get(deviceId);
    if (!ring) {
      ring = [];
      this.rings.set(deviceId, ring);
    }
    ring.push({ seq, event });
    if (ring.length > this.capacity) {
      // FIFO ring: the oldest entries age out first, so a device's ring
      // always stays a contiguous run of its stream.
      ring.splice(0, ring.length - this.capacity);
      this.evicted.add(deviceId);
    }
  }

  /**
   * The device's buffered entries after `sinceSeq`, with hole detection.
   * `deviceId` null (open/dev mode without a bound session) replays nothing.
   */
  replay(deviceId: string | null, sinceSeq: number): ReplayResult {
    if (deviceId == null) return { entries: [], complete: true };
    const ring = this.rings.get(deviceId);
    if (!ring || ring.length === 0) return { entries: [], complete: true };
    const oldest = ring[0];
    const firstRetained = oldest ? oldest.seq : 0;
    const complete = !this.evicted.has(deviceId) || sinceSeq >= firstRetained - 1;
    return {
      entries: complete ? ring.filter((e) => e.seq > sinceSeq) : [],
      complete,
    };
  }

  /** Drop a device's buffered stream entirely (revocation hygiene). */
  forget(deviceId: string): void {
    this.rings.delete(deviceId);
    this.evicted.delete(deviceId);
  }
}
