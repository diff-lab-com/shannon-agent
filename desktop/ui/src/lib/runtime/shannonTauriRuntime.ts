/**
 * P2-5a spike — Shannon ↔ assistant-ui runtime adapter.
 *
 * SCOPE FINDING — IMPORTANT
 * -------------------------
 * The original spike brief asked us to write a `class ShannonTauriRuntime
 * implements ExternalStoreRuntime<T>`. That contract does **not** exist in
 * @assistant-ui/react 0.15.x.
 *
 * What actually exists (verified against node_modules/@assistant-ui/core@0.3.2
 * runtime-cores/external-store/external-store-adapter.d.ts):
 *
 *   1. `ExternalStoreAdapter<T>` — a *config object* (not a class) with a
 *      fixed surface — `messages`, `setMessages`, `onNew`, `onEdit`,
 *      `onReload`, `onCancel`, `onAddToolResult`, etc.
 *   2. `useExternalStoreRuntime<T>(adapter)` — a React *hook* that consumes
 *      that config and returns an `AssistantRuntime`.
 *
 * So the right shape for this spike is `makeShannonTauriAdapter()` —
 * a factory returning an `ExternalStoreAdapter<ThreadMessage>` config object
 * that the page wires through `useExternalStoreRuntime(...)`.
 *
 * Production rollout (P2-5b and after) will fill in:
 *   • `messages` → backed by an in-memory `Map<sessionId, ThreadMessage[]>`
 *     synced with Tauri `query-event` payloads emitted by Rust.
 *   • `onNew` → `invoke('send_message', { message, filePaths })` via
 *     `ShannonChatModelAdapter` (which handles the streaming event bridge).
 *   • `setMessages` / `onEdit` / `onReload` → Tauri `query-control`
 *     commands.
 *
 * Spike-only: we operate on a hard-coded `MOCK_MESSAGES` list.
 */
import type {
  AppendMessage,
  ExternalStoreAdapter,
  TextMessagePart,
  ThreadMessage,
} from '@assistant-ui/react';
import { MOCK_MESSAGES } from './mockTauriEvents';
import { ShannonChatModelAdapter } from './chatModelAdapter';
import type { ShannonTauriBridge } from './shannonStream';

const SHANNON_SPIKE_TAG = '[shannonSpike]';

export interface ShannonTauriAdapterOptions {
  /**
   * Initial message list. Spike defaults to `MOCK_MESSAGES`. Production
   * wiring replaces this with a per-session store lookup.
   */
  initialMessages?: readonly ThreadMessage[];

  /**
   * Optional override of the model adapter — production will swap in a
   * real one that talks to Tauri. Tests can pass a stub.
   */
  modelAdapter?: ShannonChatModelAdapter;

  /**
   * P2-5a expansion: when supplied, the adapter is built with this
   * bridge wired in, so `onNew` actually drives a real Shannon query
   * via `invoke('send_message')` + Tauri stream events. Omit (or pass
   * `undefined`) to keep the spike's mock-driven behaviour — used by
   * `ChatV2Spike` and tests that don't need a Tauri runtime.
   */
  bridge?: ShannonTauriBridge;
}

/**
 * Build an `ExternalStoreAdapter<ThreadMessage>` suitable for
 * `useExternalStoreRuntime(...)`. Holds the (mutable during spike) copy of
 * messages in a closure-local array so it survives a single thread render.
 */
export function makeShannonTauriAdapter(
  options: ShannonTauriAdapterOptions = {},
): ExternalStoreAdapter<ThreadMessage> {
  const {
    initialMessages = MOCK_MESSAGES,
    modelAdapter,
    bridge,
  } = options;
  // Build the model adapter last so we can pass the bridge in. Default
  // to the spike adapter (no bridge) when no override is supplied.
  const resolvedModelAdapter = modelAdapter ?? new ShannonChatModelAdapter({ bridge });

  // Local mutable copy — ExternalStoreAdapter expects `messages` to be
  // observable, so we read from this array on every render and write to it
  // from setMessages/onNew/onEdit etc.
  let messages: ThreadMessage[] = [...initialMessages];

  return {
    // Read-side — assistant-ui calls this once per render. Returning a new
    // array reference triggers re-render via React's diff.
    get messages(): ThreadMessage[] {
      return messages;
    },

    // Write-side — used by assistant-ui for branch switches / state imports.
    setMessages(next: readonly ThreadMessage[]): void {
       
      console.log(`${SHANNON_SPIKE_TAG} setMessages (${next.length})`);
      messages = [...next];
    },

    async onNew(message: AppendMessage): Promise<void> {
       
      console.log(`${SHANNON_SPIKE_TAG} onNew:`, {
        parentId: message.parentId,
        sourceId: message.sourceId,
        role: message.role,
        startRun: message.startRun,
      });

      if (message.role === 'user' && message.startRun !== false) {
        await resolvedModelAdapter.run({
          messages,
          runConfig: message.runConfig ?? { custom: {} },
          abortSignal: new AbortController().signal,
          context: { system: '', tools: {} },
          // Spike returns the head message; production should track the
          // optimistic head through setMessages / append.
          unstable_getMessage() {
            const head = messages.at(-1);
            if (!head) {
              throw new Error('[shannonSpike] unstable_getMessage called on empty thread');
            }
            return head;
          },
        });
      }
    },

    async onEdit(): Promise<void> {
      // Spike no-op — production routes to Tauri `query-control edit`.
       
      console.log(`${SHANNON_SPIKE_TAG} onEdit (spike no-op)`);
    },

    async onDelete(messageId: string): Promise<void> {
      messages = messages.filter((m) => m.id !== messageId);
       
      console.log(`${SHANNON_SPIKE_TAG} onDelete: ${messageId}`);
    },

    async onReload(): Promise<void> {
       
      console.log(`${SHANNON_SPIKE_TAG} onReload (spike no-op)`);
    },

    async onCancel(): Promise<void> {
       
      console.log(`${SHANNON_SPIKE_TAG} onCancel (spike no-op)`);
    },
  } satisfies ExternalStoreAdapter<ThreadMessage>;
}

/** Re-export `MOCK_MESSAGES` so callers can `import { MOCK_MESSAGES } from '…/runtime'`.*/
export { MOCK_MESSAGES };

/** Convenience: type-safe text-extractor matching the assistant-ui part shape. */
export function readText(parts: readonly { type: string; text?: string }[]): string {
  const t = parts.find((p): p is TextMessagePart => p.type === 'text');
  return t?.text ?? '';
}
