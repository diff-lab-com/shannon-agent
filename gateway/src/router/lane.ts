import { type Logger } from "../adapters/types.js";
import { type EngineWsClient } from "../engine/wsClient.js";

/**
 * One lane per session key. Runs turns strictly in arrival order (F5: serial
 * within a conversation) while owning a single EngineWsClient for that
 * session.
 *
 * The client is created lazily on first use and memoized, so all turns in a
 * session share one engine connection. The factory receives the session key
 * so the host can pin a stable `session_id` (consumes the engine's P0-d/e
 * persistence — history survives across turns under the same id).
 *
 * `enqueue` returns a promise that resolves when this specific turn finishes,
 * but the lane's internal chain never rejects (a failed turn is logged and the
 * chain continues), so one bad turn doesn't deadlock the session.
 */
export class SessionLane {
  private tail: Promise<void> = Promise.resolve();
  private clientPromise: Promise<EngineWsClient> | null = null;

  constructor(
    readonly key: string,
    private readonly clientFactory: () => EngineWsClient,
    private readonly logger: Logger,
  ) {}

  /**
   * Connected engine client for this session. Memoized after first use.
   *
   * review F41: a failed connect must not poison the lane — the memo is
   * cleared on rejection so the next turn dials a fresh connection instead of
   * replaying the stale error forever (e.g. the engine was restarting during
   * the first turn). No backoff exists in the WS client, so one attempt per
   * turn is the retry policy.
   */
  getClient(): Promise<EngineWsClient> {
    if (this.clientPromise) return this.clientPromise;
    const attempt = (async () => {
      const client = this.clientFactory();
      await client.connect();
      this.logger.info(`lane ${this.key}: engine client connected`);
      return client;
    })();
    const memoized = attempt.catch((err: unknown) => {
      if (this.clientPromise === memoized) this.clientPromise = null;
      // Rethrow so the turn that triggered the connect still fails loudly.
      throw err;
    });
    // `memoized` is dropped after a failure (and `stop()` only reads it via
    // its own catch) — pin a no-op handler on the underlying attempt so its
    // rejection can never surface as unhandled.
    attempt.catch(() => {});
    this.clientPromise = memoized;
    return memoized;
  }

  /**
   * Run `task` after every previously enqueued task in this lane settles.
   * The returned promise reflects this task's outcome; the lane chain itself
   * stays rejection-free so subsequent turns still run.
   */
  enqueue(task: () => Promise<void>): Promise<void> {
    const previous = this.tail;
    const next = previous.then(task);
    // Swallow rejection for chain continuity; the caller observes `next`.
    this.tail = next.then(
      () => {},
      () => {},
    );
    return next;
  }

  /** Close the owned engine client, if one was created. */
  async stop(): Promise<void> {
    if (!this.clientPromise) return;
    const client = await this.clientPromise.catch(() => null);
    if (client) {
      await client.close();
      this.logger.info(`lane ${this.key}: engine client closed`);
    }
  }
}
