/**
 * P2-5a — `ChatModelAdapter` that bridges assistant-ui's `onNewMessage`
 * (a user send event) → Shannon's `invoke('send_message', …)` Tauri command.
 *
 * Behaviour:
 *   • When a `ShannonTauriBridge` is supplied, the adapter:
 *       1. calls `bridge.sendMessage(...)` to obtain a query id,
 *       2. drains `ShannonStreamEvent`s into an in-memory queue,
 *       3. emits text / tool-call parts as they arrive,
 *       4. resolves with `complete` (or `incomplete/cancelled`) once a
 *          terminal event (completed/failed/cancelled) fires.
 *   • When no bridge is supplied, the adapter falls back to the original
 *     spike behaviour: log and yield a single canned ack. This keeps the
 *     existing ChatV2Spike page green and avoids forcing every test to
 *     instantiate a bridge.
 *
 * Contract reference: @assistant-ui/core@0.3.2
 *   `ChatModelAdapter.run(options: ChatModelRunOptions):
 *      Promise<ChatModelRunResult> | AsyncGenerator<ChatModelRunResult, void>`
 *   where `ChatModelRunResult.content` is `readonly ThreadAssistantMessagePart[]`.
 *
 * Mock Tauri events live in `./mockTauriEvents.ts`.
 */
import type {
  ChatModelAdapter,
  ChatModelRunResult,
  TextMessagePart,
  ThreadAssistantMessagePart,
  ToolCallMessagePart,
} from '@assistant-ui/react';
import type {
  ShannonTauriBridge,
  ShannonStreamEvent,
} from './shannonStream';

const SHANNON_ADAPTER_TAG = '[shannonAdapter]';

const ACK_TEXT: TextMessagePart = {
  type: 'text',
  text: '(spike) ack: received',
};

/** Pull the last user-submitted message body out of the run options. The
 * adapter receives the *full* transcript; we only forward the tail to
 * the engine so the model sees the freshest prompt. */
function tailUserMessage(
  options: Parameters<ChatModelAdapter['run']>[0],
): string {
  const tail = options.messages.at(-1);
  return extractText(tail?.content) ?? '';
}

/**
 * Per-run state — keeps a transient event queue and pending status while
 * a Shannon query streams. Re-created on each `run()` so concurrent
 * threads (P2-5b) get isolated state.
 */
interface PendingRun {
  events: ShannonStreamEvent[];
  resolve: (result: ChatModelRunResult) => void;
  reject: (error: unknown) => void;
  cancelled: boolean;
}

export class ShannonChatModelAdapter implements ChatModelAdapter {
  /** `true` once `run()` has been awaited at least once. Useful in dev tools. */
  hasRun = false;

  /** Bridge against the real Tauri runtime; `undefined` means spike mode. */
  private readonly bridge: ShannonTauriBridge | undefined;

  constructor(opts: { bridge?: ShannonTauriBridge } = {}) {
    this.bridge = opts.bridge;
  }

  async run(options: Parameters<ChatModelAdapter['run']>[0]): Promise<ChatModelRunResult> {
    this.hasRun = true;
    // The last message is the user submission (assistant-ui hands us the full
    // transcript under `options.messages`, including already-rendered
    // assistant turns — we only care about the tail).
    const last = options.messages.at(-1);
    // eslint-disable-next-line no-console -- spike: intentional dev log
    console.log(`${SHANNON_ADAPTER_TAG} onNewMessage:`, {
      messageCount: options.messages.length,
      lastRole: last?.role,
      lastText: extractText(last?.content),
      runConfig: options.runConfig,
      hasBridge: this.bridge !== undefined,
    });

    // Tap the abort signal — the spike does not actually run a real query,
    // but production wiring must respect cancellation when the user aborts.
    if (options.abortSignal.aborted) {
      return { content: [], status: { type: 'incomplete', reason: 'cancelled' } };
    }

    // Spike path — preserve the original behaviour verbatim so the
    // ChatV2Spike page and any tests that rely on it keep working.
    if (!this.bridge) {
      return { content: [ACK_TEXT] };
    }

    return this.runWithBridge(options);
  }

  /**
   * Real-bridge path. Forwards the user prompt to Tauri, drains stream
   * events, and resolves once the engine emits a terminal status. The
   * returned `content` includes every text / tool-call part the engine
   * produced — assistant-ui renders these into the visible message.
   */
  private async runWithBridge(
    options: Parameters<ChatModelAdapter['run']>[0],
  ): Promise<ChatModelRunResult> {
    const prompt = tailUserMessage(options);
    const pending: PendingRun = {
      events: [],
      resolve: () => undefined,
      reject: () => undefined,
      cancelled: false,
    };

    // Drive the bridge and resolve `pending` from the terminal event.
    const terminal = new Promise<ChatModelRunResult>((resolve, reject) => {
      pending.resolve = resolve;
      pending.reject = reject;
    });

    // Hook abort → bridge.cancel() + mark cancelled so the resolver
    // returns an `incomplete/cancelled` status.
    const abort = () => {
      if (pending.cancelled) return;
      pending.cancelled = true;
      void this.bridge?.cancel();
      pending.resolve({
        content: drainEventsToContent(pending.events),
        status: { type: 'incomplete', reason: 'cancelled' },
      });
    };
    options.abortSignal.addEventListener('abort', abort, { once: true });

    // Hold the query id in a mutable cell so the handler (which can
    // fire on the next microtask, *before* the `sendMessage` promise
    // resolves) can read it for cross-thread leakage detection.
    const queryIdCell: { value: string } = { value: '' };
    try {
      queryIdCell.value = (
        await this.bridge!.sendMessage({
          message: prompt,
          filePaths: null,
          handler: (event) => {
            if (pending.cancelled) return;
            pending.events.push(event);
            if (isTerminalEvent(event)) {
              pending.resolve({
                content: drainEventsToContent(pending.events),
                status: terminalStatus(event, queryIdCell.value),
              });
            }
          },
        })
      ).query_id;
    } catch (err) {
      pending.reject(err);
    }

    options.abortSignal.removeEventListener('abort', abort);
    return terminal;
  }
}

/** Coerce a `serde_json::Value`-shaped payload into a JSON object when
 * possible. assistant-ui requires `args` to be a plain JSON object. */
function isJsonObject(value: unknown): value is Record<string, unknown> {
  return (
    typeof value === 'object' &&
    value !== null &&
    !Array.isArray(value)
  );
}

/** `true` for events that end the run. */
function isTerminalEvent(event: ShannonStreamEvent): boolean {
  return (
    event.kind === 'completed' ||
    event.kind === 'failed' ||
    event.kind === 'cancelled'
  );
}

/** Translate the terminal event into assistant-ui's `RunStatus`. */
function terminalStatus(
  event: ShannonStreamEvent,
  queryId: string,
): ChatModelRunResult['status'] {
  if (event.kind === 'completed') {
    // Drop mismatched query ids — only the engine that issued this run
    // can terminate it. Cross-thread leakage would otherwise corrupt the
    // Thread when P2-5b lands concurrent queries.
    if (queryId && event.payload.query_id !== queryId) {
      return { type: 'incomplete', reason: 'cancelled' };
    }
    return { type: 'complete', reason: 'stop' };
  }
  if (event.kind === 'cancelled') {
    return { type: 'incomplete', reason: 'cancelled' };
  }
  // Failed — narrow with an explicit alias so the union narrows on `event.kind`.
  const failed: { kind: 'failed'; payload: { error: string } } = event as {
    kind: 'failed';
    payload: { error: string };
  };
  return { type: 'incomplete', reason: 'error', error: failed.payload.error };
}

/**
 * Fold the buffered event stream into the assistant-ui message-part
 * array. Only the part-types the engine actually emits are produced:
 *
 *   text        → TextMessagePart
 *   tool-start  → ToolCallMessagePart (argsText comes from JSON.stringify)
 *   thinking    → TextMessagePart (rendered as the same shape; the UI
 *                 decides whether to fold it under a thinking block)
 *
 * Tool-result / usage / progress are metadata and do not produce their
 * own assistant-ui parts — they update existing tool-call parts in
 * follow-up events (out of scope for the P2-5a expansion; the runtime
 * surfaces them but the adapter intentionally keeps the message-part
 * contract minimal).
 */
function drainEventsToContent(
  events: readonly ShannonStreamEvent[],
): readonly ThreadAssistantMessagePart[] {
  const parts: ThreadAssistantMessagePart[] = [];
  for (const event of events) {
    switch (event.kind) {
      case 'text':
        parts.push({ type: 'text', text: event.payload.content });
        break;
      case 'thinking':
        // Surface thinking chunks as text — the spike UI folds them
        // under a thinking block; future revisions can introduce a
        // dedicated part type once assistant-ui exposes one.
        parts.push({ type: 'text', text: event.payload.content });
        break;
      case 'tool-start': {
        const rawArgs = event.payload.tool_input;
        // assistant-ui's `ToolCallMessagePart.args` is a required JSON
        // object; coerce null/non-object payloads into an empty object
        // so the UI never sees a malformed part.
        const args = isJsonObject(rawArgs) ? rawArgs : {};
        const argsText = JSON.stringify(args);
        const part: ToolCallMessagePart = {
          type: 'tool-call',
          toolCallId: event.payload.tool_use_id,
          toolName: event.payload.tool_name,
          // assistant-ui's `ReadonlyJSONObject` is structurally
          // identical to `Record<string, unknown>`; the cast is safe
          // because we coerced non-object payloads above.
          args: args as ToolCallMessagePart['args'],
          argsText,
        };
        parts.push(part);
        break;
      }
      default:
        // tool-result / tool-progress / usage / terminal events produce
        // no content parts.
        break;
    }
  }
  return parts;
}

function extractText(
  parts: ReadonlyArray<{ type: string; text?: string }> | undefined,
): string | undefined {
  if (!parts) return undefined;
  const textPart = parts.find((p) => p.type === 'text');
  return textPart?.text;
}
