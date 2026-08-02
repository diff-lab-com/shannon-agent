/**
 * P2-5a expansion — typed Tauri bridge for Shannon ↔ assistant-ui.
 *
 * Wraps the engine's `send_message` command and the streaming event
 * payloads that the Rust shell emits (`event_names::QUERY_*` /
 * `event_names::TOOL_*` constants in `crates/shannon-types/src/events.rs`,
 * re-exported from `desktop/src/events.rs`). The bridge is a thin,
 * dependency-injected seam so unit tests can drive the runtime adapter
 * without touching the real Tauri runtime.
 *
 * Wire shape reference (Rust → JS, auto-camelCased by Tauri):
 *
 *   invoke('send_message', { message, filePaths: Vec<String> })
 *     → { query_id: string }
 *
 *   listen('query:text',            { query_id, content })
 *   listen('query:tool-start',      { query_id, tool_use_id, tool_name, tool_input })
 *   listen('query:tool-result',     { query_id, tool_use_id, tool_name, result, is_error })
 *   listen('query:tool-progress',   { query_id, tool_use_id, tool_name, progress, message })
 *   listen('query:thinking',        { query_id, content })
 *   listen('query:usage',           { query_id, input_tokens, output_tokens, cost_usd })
 *   listen('query:completed',       { query_id })
 *   listen('query:failed',          { query_id, error })
 *   listen('query:cancelled',       { query_id })
 *
 * Production pages should construct a `ShannonTauriBridge` once (e.g. in
 * `ChatContext`) and pass it into `makeShannonTauriAdapter({ bridge })`.
 * The spike page still uses the no-bridge constructor (returns the legacy
 * mock-driven `ExternalStoreAdapter`) so P2-5a doesn't regress.
 */
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type {
  QueryTextPayload,
  QueryCompletedPayload,
  QueryFailedPayload,
  QueryCancelledPayload,
  ThinkingPayload,
  ToolProgressPayload,
  ToolResultPayload,
  ToolStartPayload,
  UsagePayload,
} from '@/types';
import { EVENT_NAMES } from '@/types';

/** Result payload of `invoke('send_message', …)`. */
export interface SendMessageResponse {
  query_id: string;
}

/**
 * Discriminated union over every streaming event the Rust shell emits.
 * Carrying the union (instead of one type per channel) means a single
 * subscriber can pattern-match without juggling five callback handles.
 */
export type ShannonStreamEvent =
  | { kind: 'text'; payload: QueryTextPayload }
  | { kind: 'thinking'; payload: ThinkingPayload }
  | { kind: 'tool-start'; payload: ToolStartPayload }
  | { kind: 'tool-result'; payload: ToolResultPayload }
  | { kind: 'tool-progress'; payload: ToolProgressPayload }
  | { kind: 'usage'; payload: UsagePayload }
  | { kind: 'completed'; payload: QueryCompletedPayload }
  | { kind: 'failed'; payload: QueryFailedPayload }
  | { kind: 'cancelled'; payload: QueryCancelledPayload };

export type ShannonStreamHandler = (event: ShannonStreamEvent) => void;

/**
 * Production implementation. Lives behind an interface so unit tests
 * can drop in a `MockShannonTauriBridge` without mocking the Tauri
 * modules globally.
 */
export interface ShannonTauriBridge {
  /**
   * Start streaming for a fresh `send_message` invocation. Returns the
   * query id issued by the Rust shell. Events are dispatched to `handler`
   * until `complete` (success/failure/cancel) is observed.
   *
   * Errors thrown by `invoke('send_message')` propagate to the caller —
   * the runtime adapter maps them to an `incomplete/cancelled` (or
   * `complete/error`) assistant status.
   */
  sendMessage(args: {
    message: string;
    filePaths?: readonly string[] | null;
    handler: ShannonStreamHandler;
  }): Promise<SendMessageResponse>;

  /**
   * Detach every listener registered through `sendMessage`. Safe to call
   * when no query is in flight.
   */
  cancel(): Promise<void>;
}

/** Detect mock-mode vs real Tauri at construction time. */
function isMockMode(): boolean {
  // Vite exposes VITE_MOCK_MODE in the same shape as other env vars; the
  // mock layer swaps @tauri-apps/api/core → coreMock.ts at build time.
  if (typeof import.meta !== 'undefined' && (import.meta as { env?: Record<string, string> }).env) {
    const env = (import.meta as { env: Record<string, string> }).env;
    if (env.VITE_MOCK_MODE === '1' || env.MODE === 'demo') return true;
  }
  return false;
}

/**
 * Concrete bridge against the real Tauri runtime. Each `sendMessage` call
 * installs 9 listeners and tears them down when one of `completed`,
 * `failed`, or `cancelled` fires (or when the caller invokes `cancel()`).
 */
export class RealShannonTauriBridge implements ShannonTauriBridge {
  private unlisteners: UnlistenFn[] = [];
  private activeHandler: ShannonStreamHandler | null = null;
  private readonly mock: boolean;

  constructor(opts: { mock?: boolean } = {}) {
    this.mock = opts.mock ?? isMockMode();
  }

  async sendMessage({
    message,
    filePaths,
    handler,
  }: {
    message: string;
    filePaths?: readonly string[] | null;
    handler: ShannonStreamHandler;
  }): Promise<SendMessageResponse> {
    // If a previous call never cleaned up (shouldn't happen, but the
    // Rust side can race), drop its listeners first.
    await this.teardown();

    this.activeHandler = handler;
    // Wire up listeners *before* invoking so we don't miss the first
    // chunk that the engine emits synchronously after the command runs.
    await this.installListeners();

    if (this.mock) {
      // Mock-mode path — surface a fake query id so the adapter can
      // route events; the demo handler in `coreMock.ts` never emits
      // any stream events, so we resolve a complete status immediately.
      const fakeId = `q-${Date.now()}`;
      queueMicrotask(() =>
        handler({ kind: 'completed', payload: { query_id: fakeId } }),
      );
      return { query_id: fakeId };
    }

    const response = await invoke<SendMessageResponse>('send_message', {
      message,
      filePaths: filePaths ?? null,
    });
    return response;
  }

  async cancel(): Promise<void> {
    if (this.unlisteners.length === 0) return;
    if (!this.mock) {
      // Best-effort — if no query is running, Rust returns Ok(()) and
      // we still want to clear our listeners. Failures are non-fatal.
      try {
        await invoke('cancel_query');
      } catch {
        // ignore — the listener teardown is what matters
      }
    }
    await this.teardown();
  }

  /** Install listeners and return once they're all subscribed. */
  private async installListeners(): Promise<void> {
    const handler = this.activeHandler;
    if (!handler) return;

    const bindings: Array<Promise<UnlistenFn>> = [
      listen<QueryTextPayload>(EVENT_NAMES.QUERY_TEXT, (e) =>
        handler({ kind: 'text', payload: e.payload }),
      ),
      listen<ThinkingPayload>(EVENT_NAMES.QUERY_THINKING, (e) =>
        handler({ kind: 'thinking', payload: e.payload }),
      ),
      listen<ToolStartPayload>(EVENT_NAMES.QUERY_TOOL_START, (e) =>
        handler({ kind: 'tool-start', payload: e.payload }),
      ),
      listen<ToolResultPayload>(EVENT_NAMES.QUERY_TOOL_RESULT, (e) =>
        handler({ kind: 'tool-result', payload: e.payload }),
      ),
      listen<ToolProgressPayload>(EVENT_NAMES.QUERY_TOOL_PROGRESS, (e) =>
        handler({ kind: 'tool-progress', payload: e.payload }),
      ),
      listen<UsagePayload>(EVENT_NAMES.QUERY_USAGE, (e) =>
        handler({ kind: 'usage', payload: e.payload }),
      ),
      listen<QueryCompletedPayload>(EVENT_NAMES.QUERY_COMPLETED, (e) =>
        handler({ kind: 'completed', payload: e.payload }),
      ),
      listen<QueryFailedPayload>(EVENT_NAMES.QUERY_FAILED, (e) =>
        handler({ kind: 'failed', payload: e.payload }),
      ),
      listen<QueryCancelledPayload>(EVENT_NAMES.QUERY_CANCELLED, (e) =>
        handler({ kind: 'cancelled', payload: e.payload }),
      ),
    ];

    this.unlisteners = await Promise.all(bindings);
  }

  private async teardown(): Promise<void> {
    const unlisteners = this.unlisteners;
    this.unlisteners = [];
    this.activeHandler = null;
    await Promise.all(
      unlisteners.map(async (fn) => {
        try {
          await fn();
        } catch {
          // ignore — listeners occasionally throw on teardown
        }
      }),
    );
  }
}