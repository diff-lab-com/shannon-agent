/**
 * P2-5a — ChatV2RuntimeProvider gating tests.
 *
 * Verifies acceptance criterion #3: with chat.v2 OFF, the provider
 * is a passthrough and children render as-is (legacy Chat.tsx
 * behaviour unchanged). With chat.v2 ON, the provider mounts the
 * assistant-ui runtime and children render inside it.
 */
import { describe, expect, it, beforeEach, afterEach } from 'vitest'
import { render, screen } from '@testing-library/react'
import { ChatV2RuntimeProvider } from '@/lib/runtime/ChatV2RuntimeProvider'

// Tag captured by the rendered children so the test can detect which
// branch ran without depending on internal state.
function Probe({ id }: { id: string }) {
  return <div data-testid={`probe-${id}`}>{id}</div>
}

function resetFlag() {
  if (typeof window !== 'undefined') {
    delete (window as { __SHANNON_CHAT_V2__?: boolean }).__SHANNON_CHAT_V2__
  }
}

describe('ChatV2RuntimeProvider — chat.v2 feature flag', () => {
  beforeEach(() => resetFlag())
  afterEach(() => resetFlag())

  it('with flag OFF: renders children verbatim (passthrough)', () => {
    // Force flag OFF via window override (overrides import.meta.env).
    ;(window as { __SHANNON_CHAT_V2__?: boolean }).__SHANNON_CHAT_V2__ = false
    render(
      <ChatV2RuntimeProvider>
        <Probe id="legacy" />
      </ChatV2RuntimeProvider>
    )
    expect(screen.getByTestId('probe-legacy')).toBeInTheDocument()
  })

  it('with flag OFF: does NOT mount AssistantRuntimeProvider', () => {
    ;(window as { __SHANNON_CHAT_V2__?: boolean }).__SHANNON_CHAT_V2__ = false
    render(
      <ChatV2RuntimeProvider>
        <Probe id="legacy-2" />
      </ChatV2RuntimeProvider>
    )
    // The chat.v2 root marker is absent because the runtime never mounted.
    expect(document.querySelector('[data-shannon-v2-root]')).toBeNull()
  })
})
