// GB P2-10a — the interrupt-now steering hook (round-1 review contracts).
//
//   Imp-1: the flush does NOT release the pending slot before sending —
//     hasPendingSteer() stays true through the settle commit AND the
//     delivery, so Chat's queue drain can never race in and burn a queued
//     item against the backend's concurrent-query guard.
//   Imp-2: the pending steer is parked under its session key — a settle
//     observed while another session is visible never receives it; a cancel
//     that never settles hands the draft back after settleTimeoutMs.
//
// Fake timers are active for the timeout paths, so microtask settles are
// flushed with awaited acts instead of waitFor (which polls on timers).

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { renderHook, act } from '@testing-library/react'
import {
  useSteerSend,
  type SteerAbortReason,
  type SteerSendOptions,
} from '@/hooks/useSteerSend'

function setup(overrides: Partial<SteerSendOptions> = {}) {
  const cancelQuery = vi.fn().mockResolvedValue(undefined)
  const sendMessage = vi.fn().mockResolvedValue(true)
  const onSendRejected = vi.fn()
  const utils = renderHook((props: Partial<SteerSendOptions> = {}) =>
    useSteerSend({
      visibleSessionId: 'sess-A',
      isQuerying: false,
      cancelQuery,
      sendMessage,
      onSendRejected,
      ...overrides,
      ...props,
    }),
  )
  return { ...utils, cancelQuery, sendMessage, onSendRejected }
}

/** Flush promise microtasks inside act (fake timers block waitFor's polling). */
async function flushMicrotasks(times = 3) {
  for (let i = 0; i < times; i++) {
    await act(async () => {
      await Promise.resolve()
    })
  }
}

beforeEach(() => {
  vi.useFakeTimers()
})

afterEach(() => {
  vi.useRealTimers()
})

describe('useSteerSend — park & flush', () => {
  it('while streaming: parks the message and cancels the run', () => {
    const { result, cancelQuery, sendMessage } = setup({ isQuerying: true })
    let accepted = false
    act(() => {
      accepted = result.current.steer('stop that, use Rust instead', [])
    })
    expect(accepted).toBe(true)
    expect(cancelQuery).toHaveBeenCalledTimes(1)
    expect(result.current.hasPendingSteer()).toBe(true)
    expect(sendMessage).not.toHaveBeenCalled()
  })

  it('Imp-1: hasPendingSteer stays TRUE from the settle through the delivery', async () => {
    const { result, rerender, sendMessage } = setup({ isQuerying: true })
    let resolveDelivery!: (ok: boolean) => void
    sendMessage.mockImplementation(() => new Promise<boolean>(res => { resolveDelivery = res }))
    act(() => {
      result.current.steer('now run the tests', ['/repo/a.diff'])
    })
    // The run settles — the flush effect fires and delivers synchronously…
    act(() => {
      rerender({ isQuerying: false })
    })
    expect(sendMessage).toHaveBeenCalledWith('now run the tests', ['/repo/a.diff'])
    // …but the drain gate is STILL up: the pending slot only releases when
    // the delivery RESOLVES, so the drain effect running later in this same
    // commit is guaranteed to see hasPendingSteer() === true.
    expect(result.current.hasPendingSteer()).toBe(true)
    await act(async () => {
      resolveDelivery(true)
    })
    expect(result.current.hasPendingSteer()).toBe(false)
  })

  it('a rejected flush restores the draft via onSendRejected("rejected")', async () => {
    const { result, rerender, sendMessage, onSendRejected } = setup({ isQuerying: true })
    sendMessage.mockResolvedValue(false)
    act(() => {
      result.current.steer('interrupt text', [])
    })
    act(() => {
      rerender({ isQuerying: false })
    })
    await flushMicrotasks()
    expect(onSendRejected).toHaveBeenCalledTimes(1)
    const [pending, reason] = onSendRejected.mock.calls[0] as [Parameters<NonNullable<SteerSendOptions['onSendRejected']>>[0], SteerAbortReason]
    expect(pending.text).toBe('interrupt text')
    expect(reason).toBe('rejected')
    expect(result.current.hasPendingSteer()).toBe(false)
  })

  it('empty input is a no-op (caller keeps the draft)', () => {
    const { result, cancelQuery, sendMessage } = setup({ isQuerying: true })
    let accepted = true
    act(() => {
      accepted = result.current.steer('   ', [])
    })
    expect(accepted).toBe(false)
    expect(cancelQuery).not.toHaveBeenCalled()
    expect(sendMessage).not.toHaveBeenCalled()
    expect(result.current.hasPendingSteer()).toBe(false)
  })

  it('idle steer degrades to an ordinary send (cancel untouched)', async () => {
    const { result, cancelQuery, sendMessage, onSendRejected } = setup({ isQuerying: false })
    let accepted = true
    act(() => {
      accepted = result.current.steer('normal message', [])
    })
    expect(accepted).toBe(true)
    expect(cancelQuery).not.toHaveBeenCalled()
    await flushMicrotasks()
    expect(sendMessage).toHaveBeenCalledWith('normal message', undefined)
    expect(onSendRejected).not.toHaveBeenCalled()
  })
})

describe('useSteerSend — Imp-2 session scoping', () => {
  it('a settle on ANOTHER session never receives the parked steer; returning delivers it', async () => {
    const { result, rerender, sendMessage } = setup({ isQuerying: true })
    act(() => {
      result.current.steer('this belongs to A', [])
    })
    // The user switches to session B and A's run settles there.
    act(() => {
      rerender({ visibleSessionId: 'sess-B', isQuerying: false })
    })
    await flushMicrotasks()
    expect(sendMessage).not.toHaveBeenCalled()
    // The gate is session-scoped: A's parked steer must not block B's drain.
    expect(result.current.hasPendingSteer()).toBe(false)
    // Back on A: the parked steer delivers.
    act(() => {
      rerender({ visibleSessionId: 'sess-A', isQuerying: false })
    })
    await flushMicrotasks()
    expect(sendMessage).toHaveBeenCalledWith('this belongs to A', undefined)
  })
})

describe('useSteerSend — Imp-2 settle timeout', () => {
  it('a cancel that never settles restores the draft after the timeout (with notice)', () => {
    const { result, onSendRejected } = setup({
      isQuerying: true,
      settleTimeoutMs: 15_000,
    })
    act(() => {
      result.current.steer('interrupt text', [])
    })
    act(() => {
      vi.advanceTimersByTime(14_999)
    })
    expect(onSendRejected).not.toHaveBeenCalled()
    expect(result.current.hasPendingSteer()).toBe(true)
    act(() => {
      vi.advanceTimersByTime(1)
    })
    expect(onSendRejected).toHaveBeenCalledTimes(1)
    const [pending, reason] = onSendRejected.mock.calls[0] as [Parameters<NonNullable<SteerSendOptions['onSendRejected']>>[0], SteerAbortReason]
    expect(pending.text).toBe('interrupt text')
    expect(reason).toBe('timeout')
    expect(result.current.hasPendingSteer()).toBe(false)
  })

  it('a steer parked for an off-screen session outlives the timeout (re-armed, not aborted)', async () => {
    const { result, onSendRejected, rerender, sendMessage } = setup({
      isQuerying: true,
      settleTimeoutMs: 15_000,
    })
    act(() => {
      result.current.steer('belongs to A', [])
    })
    act(() => {
      rerender({ visibleSessionId: 'sess-B', isQuerying: false })
    })
    act(() => {
      vi.advanceTimersByTime(60_000)
    })
    // Not a dead wait the user is watching — the text stays parked and
    // delivers when A comes back.
    expect(onSendRejected).not.toHaveBeenCalled()
    act(() => {
      rerender({ visibleSessionId: 'sess-A', isQuerying: false })
    })
    await flushMicrotasks()
    expect(sendMessage).toHaveBeenCalledWith('belongs to A', undefined)
  })
})
