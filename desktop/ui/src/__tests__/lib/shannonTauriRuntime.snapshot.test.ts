/**
 * P2-5a — insta-style snapshot for the runtime adapter's initial state.
 *
 * The plan mandates interface-drift detection: as the assistant-ui
 * `ExternalStoreAdapter<ThreadMessage>` and `ChatModelAdapter`
 * contracts evolve across @assistant-ui/react point releases, the
 * shape of the initial state we hand them must remain stable. Rather
 * than pulling Rust's `insta` crate into a TS test, we use vitest's
 * built-in `toMatchSnapshot` — same idea, JS-friendly, zero deps.
 *
 * What we lock in:
 *   • The structural keys of the `ExternalStoreAdapter` returned by
 *     `makeShannonTauriAdapter()`.
 *   • The structural keys of a `ShannonStreamEvent` discriminated
 *     union member (the canonical `text` payload).
 *   • The keys of a single `ThreadMessage` produced by the mock
 *     list used by `ChatV2Spike`.
 *
 * Failures here are a "stop-the-line" signal: investigate, then
 * `pnpm exec vitest run -u` to regenerate.
 */
import { describe, expect, it } from 'vitest'
import { makeShannonTauriAdapter } from '@/lib/runtime/shannonTauriRuntime'
import { MOCK_MESSAGES } from '@/lib/runtime/mockTauriEvents'
import type { ShannonStreamEvent } from '@/lib/runtime/shannonStream'

describe('P2-5a runtime adapter — initial state shape (snapshot)', () => {
  it('exposes the expected ExternalStoreAdapter keys', () => {
    const adapter = makeShannonTauriAdapter()
    expect(Object.keys(adapter).sort()).toMatchSnapshot('shannonTauriAdapter.keys')
  })

  it('produces the expected default mock messages', () => {
    // We compare against a shape-stable projection (id + role + part types)
    // so the snapshot doesn't churn on `createdAt` timezone shifts.
    const shape = MOCK_MESSAGES.map((m) => ({
      id: m.id,
      role: m.role,
      parts: m.content.map((p) => p.type),
    }))
    expect(shape).toMatchSnapshot('mockMessages.shape')
  })

  it('discriminated ShannonStreamEvent union is stable', () => {
    const events: ShannonStreamEvent[] = [
      { kind: 'text', payload: { query_id: 'q', content: '' } },
      { kind: 'thinking', payload: { query_id: 'q', content: '' } },
      {
        kind: 'tool-start',
        payload: {
          query_id: 'q',
          tool_use_id: 'tu',
          tool_name: 'bash',
          tool_input: {},
        },
      },
      {
        kind: 'tool-result',
        payload: { query_id: 'q', tool_use_id: 'tu', tool_name: 'bash', result: '', is_error: false },
      },
      {
        kind: 'tool-progress',
        payload: { query_id: 'q', tool_use_id: 'tu', tool_name: 'bash', progress: 0, message: '' },
      },
      { kind: 'usage', payload: { query_id: 'q', input_tokens: 0, output_tokens: 0, cost_usd: 0 } },
      { kind: 'completed', payload: { query_id: 'q' } },
      { kind: 'failed', payload: { query_id: 'q', error: '' } },
      { kind: 'cancelled', payload: { query_id: 'q' } },
    ]
    const kinds = events.map((e) => e.kind).sort()
    expect(kinds).toMatchSnapshot('shannonStreamEvent.kinds')
  })
})
