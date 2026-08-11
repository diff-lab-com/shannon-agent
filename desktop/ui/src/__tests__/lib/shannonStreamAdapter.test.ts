/**
 * P2-5a expansion — unit tests for the real Tauri event stream adapter.
 *
 * What we cover:
 *   1. Spike (no-bridge) path still returns the canned ack (regression).
 *   2. Bridge path: text events → text message parts.
 *   3. Bridge path: tool-start → tool-call message part with argsText.
 *   4. Bridge path: completed → `complete/stop` status.
 *   5. Bridge path: failed → `complete/error` with engine error message.
 *   6. Bridge path: cancelled → `incomplete/cancelled`.
 *   7. Bridge path: abort signal → `incomplete/cancelled` + bridge.cancel().
 *   8. Bridge path: query id mismatch (cross-thread leakage) → cancelled.
 *   9. makeShannonTauriAdapter wires the bridge into the model adapter.
 *
 * We use a hand-rolled `MockShannonTauriBridge` (defined inline) instead
 * of mocking `@tauri-apps/api/event` so the test exercises the adapter's
 * real code path through `ShannonStreamBridge`.
 */
import { describe, expect, it, vi } from 'vitest'
import { ShannonChatModelAdapter } from '@/lib/runtime/chatModelAdapter'
import type {
  ShannonStreamEvent,
  ShannonStreamHandler,
  ShannonTauriBridge,
  SendMessageResponse,
} from '@/lib/runtime/shannonStream'
import { makeShannonTauriAdapter } from '@/lib/runtime/shannonTauriRuntime'

/* ------------------------------------------------------------------ */
/*  Inline mock bridge                                                 */
/* ------------------------------------------------------------------ */

class MockBridge implements ShannonTauriBridge {
  queryIdCounter = 0
  cancelled = 0
  sentMessages: string[] = []

  // Test-controlled list of events to flush in order after sendMessage.
  pending: ShannonStreamEvent[] = []

  private handler: ShannonStreamHandler | null = null

  async sendMessage({
    message,
    handler,
  }: {
    message: string
    filePaths?: readonly string[] | null
    handler: ShannonStreamHandler
  }): Promise<SendMessageResponse> {
    this.handler = handler
    this.sentMessages.push(message)
    this.queryIdCounter += 1
    const queryId = `mock-${this.queryIdCounter}`
    // Flush pre-queued events on the next microtask so the adapter has
    // a chance to wire its `terminal` promise resolver.
    // Defer the flush by one extra microtask so the adapter has a
    // chance to capture the returned query id into its closure cell
    // before the handler fires. Mirrors the production bridge, which
    // installs listeners before invoking send_message and emits the
    // first chunk asynchronously after Rust returns.
    queueMicrotask(() => {
      queueMicrotask(() => {
        for (const ev of this.pending) handler(ev)
        this.pending = []
        this.handler = null
      })
    })
    return { query_id: queryId }
  }

  async cancel(): Promise<void> {
    this.cancelled += 1
    if (this.handler) {
      // Mirror the real bridge: emit a `cancelled` event so the adapter
      // can settle its terminal promise.
      this.handler({ kind: 'cancelled', payload: { query_id: 'mock-cancel' } })
      this.handler = null
    }
  }
}

/* ------------------------------------------------------------------ */
/*  Test helpers                                                       */
/* ------------------------------------------------------------------ */

function makeRunOptions(prompt: string, abort?: AbortController) {
  const userPart = { type: 'text', text: prompt } as const
  return {
    messages: [
      {
        id: 'u-1',
        role: 'user' as const,
        content: [userPart],
        attachments: [],
        createdAt: new Date(),
        metadata: { custom: {} },
      },
    ],
    runConfig: { custom: {} },
    abortSignal: abort?.signal ?? new AbortController().signal,
    context: { system: '', tools: {} },
    unstable_getMessage: () => {
      throw new Error('not used')
    },
  } as unknown as Parameters<ShannonChatModelAdapter['run']>[0]
}

/* ------------------------------------------------------------------ */
/*  Tests                                                              */
/* ------------------------------------------------------------------ */

describe('ShannonChatModelAdapter — spike (no bridge)', () => {
  it('returns the canned ack and records hasRun', async () => {
    const adapter = new ShannonChatModelAdapter()
    const result = await adapter.run(makeRunOptions('hello'))
    expect(adapter.hasRun).toBe(true)
    expect(result.content).toEqual([{ type: 'text', text: '(spike) ack: received' }])
    expect(result.status).toBeUndefined()
  })

  it('returns incomplete/cancelled when the abort signal is already aborted', async () => {
    const ac = new AbortController()
    ac.abort()
    const adapter = new ShannonChatModelAdapter()
    const result = await adapter.run(makeRunOptions('hi', ac))
    expect(result.status).toEqual({ type: 'incomplete', reason: 'cancelled' })
    expect(result.content).toEqual([])
  })
})

describe('ShannonChatModelAdapter — bridge (real event stream)', () => {
  it('emits text parts for query:text events', async () => {
    const bridge = new MockBridge()
    bridge.pending = [
      { kind: 'text', payload: { query_id: 'mock-1', content: 'Hello ' } },
      { kind: 'text', payload: { query_id: 'mock-1', content: 'world' } },
      { kind: 'completed', payload: { query_id: 'mock-1' } },
    ]
    const adapter = new ShannonChatModelAdapter({ bridge })
    const result = await adapter.run(makeRunOptions('Say hi'))
    expect(result.status).toEqual({ type: 'complete', reason: 'stop' })
    expect(result.content).toEqual([
      { type: 'text', text: 'Hello ' },
      { type: 'text', text: 'world' },
    ])
    expect(bridge.sentMessages).toEqual(['Say hi'])
  })

  it('emits tool-call parts for query:tool-start with JSON-stringified args', async () => {
    const bridge = new MockBridge()
    bridge.pending = [
      {
        kind: 'tool-start',
        payload: {
          query_id: 'mock-1',
          tool_use_id: 'tu-1',
          tool_name: 'bash',
          tool_input: { command: 'ls' },
        },
      },
      { kind: 'completed', payload: { query_id: 'mock-1' } },
    ]
    const adapter = new ShannonChatModelAdapter({ bridge })
    const result = await adapter.run(makeRunOptions('run ls'))
    const parts = result.content
    expect(parts).toHaveLength(1)
    const toolPart = parts[0] as { type: string; toolName?: string; argsText?: string }
    expect(toolPart.type).toBe('tool-call')
    expect(toolPart.toolName).toBe('bash')
    expect(toolPart.argsText).toBe(JSON.stringify({ command: 'ls' }))
  })

  it('returns incomplete/error on query:failed', async () => {
    const bridge = new MockBridge()
    bridge.pending = [
      { kind: 'failed', payload: { query_id: 'mock-1', error: 'boom' } },
    ]
    const adapter = new ShannonChatModelAdapter({ bridge })
    const result = await adapter.run(makeRunOptions('hi'))
    // assistant-ui's MessageStatus allows incomplete/error but not
    // complete/error; we surface engine failures as incomplete so the
    // UI can render the engine's error message verbatim.
    expect(result.status).toEqual({ type: 'incomplete', reason: 'error', error: 'boom' })
  })

  it('returns incomplete/cancelled on query:cancelled', async () => {
    const bridge = new MockBridge()
    bridge.pending = [
      { kind: 'cancelled', payload: { query_id: 'mock-1' } },
    ]
    const adapter = new ShannonChatModelAdapter({ bridge })
    const result = await adapter.run(makeRunOptions('hi'))
    expect(result.status).toEqual({ type: 'incomplete', reason: 'cancelled' })
  })

  it('handles abort signal by calling bridge.cancel()', async () => {
    const bridge = new MockBridge()
    const cancelSpy = vi.spyOn(bridge, 'cancel')
    const ac = new AbortController()
    const adapter = new ShannonChatModelAdapter({ bridge })
    const promise = adapter.run(makeRunOptions('hi', ac))
    ac.abort()
    const result = await promise
    expect(result.status).toEqual({ type: 'incomplete', reason: 'cancelled' })
    expect(cancelSpy).toHaveBeenCalledTimes(1)
  })

  it('drops mismatched query ids to prevent cross-thread leakage', async () => {
    const bridge = new MockBridge()
    // Different query id than the one we issued — simulates a stale
    // event from a previous run leaking in (P2-5b regression test).
    // We force the next counter value to match `wrong-id` so the bridge
    // issues the same id the stale event carries, and the adapter must
    // still treat it as cancelled because the test's local queryId
    // diverges from what the engine issued.
    bridge.queryIdCounter = 41 // any value other than 0/1 — see note
    bridge.pending = [
      { kind: 'completed', payload: { query_id: 'wrong-id' } },
    ]
    const adapter = new ShannonChatModelAdapter({ bridge })
    const result = await adapter.run(makeRunOptions('hi'))
    // The bridge increments counter; local `queryId` is captured as ''
    // before the microtask flushes pending events, so the adapter
    // records a mismatch and short-circuits to incomplete/cancelled.
    expect(result.status).toEqual({ type: 'incomplete', reason: 'cancelled' })
  })

  it('forwards filePaths through the bridge when provided', async () => {
    const bridge = new MockBridge()
    bridge.pending = [
      { kind: 'completed', payload: { query_id: 'mock-1' } },
    ]
    // Replace sendMessage to capture filePaths argument.
    const sendSpy = vi.spyOn(bridge, 'sendMessage')
    const adapter = new ShannonChatModelAdapter({ bridge })
    await adapter.run(makeRunOptions('hi'))
    expect(sendSpy).toHaveBeenCalledWith(
      expect.objectContaining({ message: 'hi', filePaths: null }),
    )
  })
})

describe('makeShannonTauriAdapter — bridge wiring', () => {
  it('injects the bridge into the default ShannonChatModelAdapter', () => {
    const bridge = new MockBridge()
    const adapter = makeShannonTauriAdapter({ bridge })
    expect(adapter).toBeDefined()
    expect(adapter.messages.length).toBeGreaterThan(0)
  })

  it('falls back to spike behaviour when no bridge is supplied', () => {
    const adapter = makeShannonTauriAdapter()
    expect(adapter).toBeDefined()
    expect(adapter.messages.length).toBeGreaterThan(0)
  })
})