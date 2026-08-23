/**
 * P2-5a expansion — focused tests for `RealShannonTauriBridge`.
 *
 * The global `__tests__/setup.ts` already mocks `@tauri-apps/api/event`
 * with `listen` → `() => Promise<UnlistenFn>`. We override per-test to
 * capture which events the bridge subscribes to and which payloads are
 * dispatched to the handler.
 */
import { describe, expect, it, vi } from 'vitest'
import { listen } from '@tauri-apps/api/event'
import { invoke } from '@tauri-apps/api/core'
import { RealShannonTauriBridge } from '@/lib/runtime/tauriBridge'
import { EVENT_NAMES } from '@/types'

// Capture every `listen()` invocation and the matching handler so we can
// drive them deterministically from tests.
type CapturedHandler = (payload: unknown) => void
const captured: Record<string, CapturedHandler[]> = {}

vi.mocked(listen).mockImplementation(((event: string, handler: (e: { payload: unknown }) => void) => {
  captured[event] ??= []
  captured[event].push((payload) => handler({ payload }))
  return Promise.resolve(() => {
    if (captured[event]) {
      captured[event] = captured[event].filter((h) => h !== handler)
    }
  })
}) as unknown as typeof listen)

function flushCaptured(event: string, payload: unknown) {
  for (const h of captured[event] ?? []) h(payload)
}

describe('RealShannonTauriBridge (mocked Tauri event module)', () => {
  it('subscribes to all 9 query:* events before invoking send_message', async () => {
    const bridge = new RealShannonTauriBridge({ mock: false })
    const events: string[] = []
    vi.mocked(listen).mockImplementation(((event: string, handler: (e: { payload: unknown }) => void) => {
      events.push(event)
      captured[event] ??= []
      captured[event].push((payload) => handler({ payload }))
      return Promise.resolve(() => undefined)
    }) as unknown as typeof listen)
    vi.mocked(invoke).mockResolvedValueOnce({ query_id: 'q-1' })

    await bridge.sendMessage({
      message: 'hi',
      handler: () => undefined,
    })

    // Confirm every streaming channel we expect was bound.
    for (const name of [
      EVENT_NAMES.QUERY_TEXT,
      EVENT_NAMES.QUERY_THINKING,
      EVENT_NAMES.QUERY_TOOL_START,
      EVENT_NAMES.QUERY_TOOL_RESULT,
      EVENT_NAMES.QUERY_TOOL_PROGRESS,
      EVENT_NAMES.QUERY_USAGE,
      EVENT_NAMES.QUERY_COMPLETED,
      EVENT_NAMES.QUERY_FAILED,
      EVENT_NAMES.QUERY_CANCELLED,
    ]) {
      expect(events).toContain(name)
    }

    // And the engine was invoked with the correct wire shape.
    expect(invoke).toHaveBeenCalledWith('send_message', {
      message: 'hi',
      filePaths: null,
    })
  })

  it('routes a query:text payload to the handler with the right shape', async () => {
    const bridge = new RealShannonTauriBridge({ mock: false })
    const handler = vi.fn()
    vi.mocked(invoke).mockResolvedValueOnce({ query_id: 'q-1' })
    await bridge.sendMessage({ message: 'hi', handler })
    flushCaptured(EVENT_NAMES.QUERY_TEXT, { query_id: 'q-1', content: 'piece ' })
    flushCaptured(EVENT_NAMES.QUERY_TEXT, { query_id: 'q-1', content: 'of text' })
    expect(handler).toHaveBeenCalledWith({
      kind: 'text',
      payload: { query_id: 'q-1', content: 'piece ' },
    })
    expect(handler).toHaveBeenCalledWith({
      kind: 'text',
      payload: { query_id: 'q-1', content: 'of text' },
    })
  })

  it('routes a query:tool-start payload', async () => {
    const bridge = new RealShannonTauriBridge({ mock: false })
    const handler = vi.fn()
    vi.mocked(invoke).mockResolvedValueOnce({ query_id: 'q-1' })
    await bridge.sendMessage({ message: 'hi', handler })
    flushCaptured(EVENT_NAMES.QUERY_TOOL_START, {
      query_id: 'q-1',
      tool_use_id: 'tu-1',
      tool_name: 'bash',
      tool_input: { command: 'ls' },
    })
    expect(handler).toHaveBeenCalledWith({
      kind: 'tool-start',
      payload: {
        query_id: 'q-1',
        tool_use_id: 'tu-1',
        tool_name: 'bash',
        tool_input: { command: 'ls' },
      },
    })
  })

  it('routes a query:failed payload', async () => {
    const bridge = new RealShannonTauriBridge({ mock: false })
    const handler = vi.fn()
    vi.mocked(invoke).mockResolvedValueOnce({ query_id: 'q-1' })
    await bridge.sendMessage({ message: 'hi', handler })
    flushCaptured(EVENT_NAMES.QUERY_FAILED, { query_id: 'q-1', error: 'boom' })
    expect(handler).toHaveBeenCalledWith({
      kind: 'failed',
      payload: { query_id: 'q-1', error: 'boom' },
    })
  })

  it('cancel() invokes cancel_query and detaches listeners', async () => {
    const bridge = new RealShannonTauriBridge({ mock: false })
    vi.mocked(invoke).mockResolvedValueOnce({ query_id: 'q-1' })
    await bridge.sendMessage({ message: 'hi', handler: () => undefined })
    vi.mocked(invoke).mockResolvedValueOnce(undefined)
    await bridge.cancel()
    expect(invoke).toHaveBeenCalledWith('cancel_query')
    // After cancel, listeners are torn down — subsequent flush is a no-op.
    flushCaptured(EVENT_NAMES.QUERY_TEXT, { query_id: 'q-1', content: 'late' })
  })

  it('mock mode surfaces a fake query id and skips invoke', async () => {
    // Reset invoke spy so the assertion below isn't polluted by prior tests.
    vi.mocked(invoke).mockClear()
    const bridge = new RealShannonTauriBridge({ mock: true })
    const handler = vi.fn()
    const response = await bridge.sendMessage({ message: 'hi', handler })
    expect(response.query_id).toMatch(/^q-\d+$/)
    expect(invoke).not.toHaveBeenCalled()
    // Allow the microtask to flush.
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(handler).toHaveBeenCalledWith(
      expect.objectContaining({ kind: 'completed' }),
    )
  })
})