/**
 * Shared in-flight engine-query registry — the "which engine client is
 * currently running a turn for this session key" map, extracted from the
 * engine bridge so MORE than one producer can register against it:
 *
 *  - the engine bridge's direct `shannon/query` clients (key = the bound
 *    device session id, or `__anon__`), and
 *  - the dispatch pipeline's per-lane clients (key = the router session key,
 *    `mobile:<deviceId>` — see `sessionKeyOf`), registered by the
 *    `SessionRouter` for the duration of each turn.
 *
 * Both sides share ONE instance (the composer creates it once and injects it,
 * the same pattern as the §L2 approval registry), so `shannon/cancel` finds a
 * dispatched task's engine turn too — previously those lane clients were
 * invisible to cancel and a dispatched task could not be interrupted.
 *
 * Semantics stay the engine bridge's original Map contract: one in-flight
 * query per key at a time (a lane serializes its turns; the bridge replaces a
 * stale client before starting fresh), entries live only for the turn's
 * duration and are removed on completion, failure and cancellation alike.
 */

/** The minimal client surface the registry needs. `EngineWsClient` and the
 *  engine bridge's `EngineClient` both satisfy it structurally. */
export interface CancelableQueryClient {
  /** Interrupt the in-flight turn (engine WS `cancel` frame). */
  cancel(): void;
  close(): Promise<void>;
}

export class ActiveQueryRegistry {
  private readonly entries = new Map<string, CancelableQueryClient>();

  /** Register the in-flight client for `key` (at most one per key). */
  set(key: string, client: CancelableQueryClient): void {
    this.entries.set(key, client);
  }

  /** Drop the entry for `key` (the turn reached a terminal). True when one was registered. */
  delete(key: string): boolean {
    return this.entries.delete(key);
  }

  /** The in-flight client for `key`, if any. */
  get(key: string): CancelableQueryClient | null {
    return this.entries.get(key) ?? null;
  }

  /** Number of in-flight queries (diagnostics / tests). */
  get size(): number {
    return this.entries.size;
  }
}

/**
 * The dispatch pipeline's per-device lane session key (`mobile:<deviceId>`).
 * `shannon/cancel` probes this alias after the bare device session id so a
 * paired device's cancel reaches a dispatched task's lane client even though
 * the engine bridge registers its own direct queries under the bare id.
 */
export function deviceLaneKey(deviceId: string): string {
  return `mobile:${deviceId}`;
}
