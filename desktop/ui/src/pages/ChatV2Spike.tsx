/**
 * P2-5a spike — test page rendering `<Thread>` driven by
 * `makeShannonTauriAdapter()` with three mocked messages.
 *
 * IMPORTANT: this page is *not* wired into production routing. App.tsx adds
 * `/chat-v2-spike` only when `import.meta.env.DEV` is true (dev-only).
 * Production `Chat.tsx` is untouched.
 *
 * SCOPE FINDING — `<Thread>` is **not** a top-level export in
 * @assistant-ui/react 0.15.x. The 0.15.x public surface exposes
 * composable `ThreadPrimitive.*` slots (Root, Viewport, ViewportFooter,
 * Messages, MessageByIndex, Composer, etc.). This page composes those slots
 * directly — the goal of the spike is to verify the runtime contract, not
 * to ship the polished thread UI.
 */
import {
  AssistantRuntimeProvider,
  ComposerPrimitive,
  MessagePrimitive,
  ThreadPrimitive,
  useExternalStoreRuntime,
} from '@assistant-ui/react';
import { makeShannonTauriAdapter } from '../lib/runtime/shannonTauriRuntime';

/** Visual marker so this page is unmistakably the spike. */
const SPIKE_BANNER = '[shannon P2-5a spike]';

export default function ChatV2Spike() {
  // `useExternalStoreRuntime<T>` takes an `ExternalStoreAdapter<T>` config
  // and returns an `AssistantRuntime`. See api-report in
  // node_modules/@assistant-ui/core/dist/react/runtimes/useExternalStoreRuntime.d.ts.
  const adapter = makeShannonTauriAdapter();
  const runtime = useExternalStoreRuntime(adapter);

  return (
    <AssistantRuntimeProvider runtime={runtime}>
      <main data-shannon-spike="true" className="flex h-full flex-col gap-4 p-4 text-foreground">
        <header className="rounded-md border border-yellow-500/40 bg-yellow-500/10 p-3 text-sm">
          <strong>{SPIKE_BANNER}</strong>{' '}
          — Shannon ↔ assistant-ui runtime adapter spike. Production chat is on{' '}
          <code className="rounded bg-muted px-1">/chat</code>. Open the browser
          devtools console for adapter logs.
        </header>

        <ThreadPrimitive.Root className="flex flex-1 flex-col gap-2 overflow-hidden rounded-md border border-border bg-background">
          <ThreadPrimitive.Viewport className="flex flex-1 flex-col gap-3 overflow-y-auto p-3">
            <ThreadPrimitive.Empty>
              <div className="text-sm text-muted-foreground">No messages yet.</div>
            </ThreadPrimitive.Empty>

            <ThreadPrimitive.Messages
              components={{
                UserMessage,
                AssistantMessage,
              }}
            />
          </ThreadPrimitive.Viewport>

          <ThreadPrimitive.ViewportFooter className="border-t border-border p-3">
            <ComposerPrimitive.Root className="flex items-center gap-2">
              <ComposerPrimitive.Input
                rows={1}
                autoFocus
                placeholder="Type a spike message…"
                className="flex-1 resize-none rounded-md border border-input bg-background px-3 py-2 text-sm outline-none focus-visible:ring-1 focus-visible:ring-ring"
              />
              <ComposerPrimitive.Send
                className="rounded-md bg-primary px-3 py-1 text-sm text-primary-foreground disabled:opacity-50"
                disabled={false}
              >
                Send
              </ComposerPrimitive.Send>
            </ComposerPrimitive.Root>
          </ThreadPrimitive.ViewportFooter>
        </ThreadPrimitive.Root>
      </main>
    </AssistantRuntimeProvider>
  );
}

// --- Minimal presentational slots to render mocked content. ----------------
//
// Production rollout will replace these with the real `assistant-ui` CLI-installed
// shadcn theme components (which the README mentions). For the spike we render
// raw text/parts so we can confirm the runtime contract.

function UserMessage() {
  return (
    <div className="rounded-md bg-muted/40 p-2 text-sm">
      <MessagePrimitive.Root>
        <MessagePrimitive.Parts />
      </MessagePrimitive.Root>
    </div>
  );
}

function AssistantMessage() {
  return (
    <div className="rounded-md border border-border p-2 text-sm">
      <MessagePrimitive.Root>
        <MessagePrimitive.Parts />
      </MessagePrimitive.Root>
    </div>
  );
}
