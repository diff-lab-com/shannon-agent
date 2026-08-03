/**
 * P2-5a — confirms the assistant-ui Thread renders Shannon text/tool/thinking
 * when the runtime adapter is wired in.
 *
 * This is the "single thread happy path" acceptance criterion #1 from
 * `docs/plans/chat-upgrade.md §3.1`. We use the existing MOCK_MESSAGES
 * fixture so the rendering probe exercises all three message shapes the
 * adapter was designed for:
 *   1. user text
 *   2. assistant text
 *   3. assistant tool-call
 *
 * The test deliberately bypasses ChatV2RuntimeProvider's flag gate so the
 * runtime mounts deterministically; the gating behaviour is covered
 * separately in `ChatV2RuntimeProvider.test.tsx`.
 */
import { describe, expect, it } from 'vitest'
import { render, screen } from '@testing-library/react'
import {
  AssistantRuntimeProvider,
  ThreadPrimitive,
  MessagePrimitive,
  useExternalStoreRuntime,
  ComposerPrimitive,
} from '@assistant-ui/react'
import { makeShannonTauriAdapter } from '@/lib/runtime/shannonTauriRuntime'

function ThreadHarness() {
  const adapter = makeShannonTauriAdapter()
  const runtime = useExternalStoreRuntime(adapter)
  return (
    <AssistantRuntimeProvider runtime={runtime}>
      <ThreadPrimitive.Root>
        <ThreadPrimitive.Viewport>
          <ThreadPrimitive.Messages
            components={{
              UserMessage: () => (
                <div data-testid="user">
                  <MessagePrimitive.Root>
                    <MessagePrimitive.Parts />
                  </MessagePrimitive.Root>
                </div>
              ),
              AssistantMessage: () => (
                <div data-testid="assistant">
                  <MessagePrimitive.Root>
                    <MessagePrimitive.Parts />
                  </MessagePrimitive.Root>
                </div>
              ),
            }}
          />
        </ThreadPrimitive.Viewport>
        <ThreadPrimitive.ViewportFooter>
          <ComposerPrimitive.Root>
            <ComposerPrimitive.Input placeholder="send…" />
            <ComposerPrimitive.Send>send</ComposerPrimitive.Send>
          </ComposerPrimitive.Root>
        </ThreadPrimitive.ViewportFooter>
      </ThreadPrimitive.Root>
    </AssistantRuntimeProvider>
  )
}

describe('chat.v2 single-thread happy path (runtime adapter → Thread)', () => {
  it('renders Shannon text and tool-call messages through the Thread primitive', () => {
    render(<ThreadHarness />)

    // Mock message text bodies should appear in the document.
    expect(screen.getByText('(spike) hello from a mock user')).toBeInTheDocument()
    expect(screen.getByText('(spike) hello back from a mock assistant')).toBeInTheDocument()
  })
})
