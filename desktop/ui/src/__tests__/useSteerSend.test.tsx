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
//   B1-1 (P0-1): the park is a per-session FIFO, not a single global slot —
//     parks for different sessions coexist, same-session parks queue in
//     park order, a park landing mid-delivery outlives that delivery's
//     cleanup, and a timeout returns the whole still-waiting batch of the
//     visible session (in-flight head excepted — its send promise owns it).
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

describe('useSteerSend — B1-1 per-session FIFO', () => {
  it('steers parked for A and B both deliver, each to its own session (no slot overwrite)', async () => {
    const { result, rerender, sendMessage } = setup({ isQuerying: true })
    act(() => {
      result.current.steer('belongs to A', [])
    })
    // B is streaming too when the user lands there and steers.
    act(() => {
      rerender({ visibleSessionId: 'sess-B', isQuerying: true })
    })
    act(() => {
      result.current.steer('belongs to B', [])
    })
    expect(sendMessage).not.toHaveBeenCalled()
    // B settles first (B is on screen): B's steer goes out, A's stays parked.
    act(() => {
      rerender({ visibleSessionId: 'sess-B', isQuerying: false })
    })
    await flushMicrotasks()
    expect(sendMessage).toHaveBeenCalledTimes(1)
    expect(sendMessage).toHaveBeenCalledWith('belongs to B', undefined)
    // A's park survives the delivery — but the gate stays session-scoped:
    // it must not block B's queue drain.
    expect(result.current.hasPendingSteer()).toBe(false)
    // Back on A (idle): A's steer delivers — the old single slot lost it.
    act(() => {
      rerender({ visibleSessionId: 'sess-A', isQuerying: false })
    })
    await flushMicrotasks()
    expect(sendMessage).toHaveBeenCalledTimes(2)
    expect(sendMessage).toHaveBeenCalledWith('belongs to A', undefined)
    expect(result.current.hasPendingSteer()).toBe(false)
  })

  it('two steers for the SAME session deliver in park order (FIFO, not overwrite)', async () => {
    const { result, rerender, sendMessage } = setup({ isQuerying: true })
    act(() => {
      result.current.steer('first interrupt', [])
    })
    act(() => {
      result.current.steer('second interrupt', [])
    })
    // The run settles: the head delivers, the tail stays queued — the gate
    // must stay up until the tail is gone too.
    act(() => {
      rerender({ isQuerying: false })
    })
    expect(sendMessage).toHaveBeenCalledTimes(1)
    expect(sendMessage).toHaveBeenNthCalledWith(1, 'first interrupt', undefined)
    expect(result.current.hasPendingSteer()).toBe(true)
    // The delivery starts a new run; when THAT settles, the tail delivers.
    await act(async () => {})
    act(() => {
      rerender({ isQuerying: true })
    })
    act(() => {
      rerender({ isQuerying: false })
    })
    expect(sendMessage).toHaveBeenCalledTimes(2)
    expect(sendMessage).toHaveBeenNthCalledWith(2, 'second interrupt', undefined)
    // Imp-1 still holds per entry: the gate only drops when the delivery
    // RESOLVES, not when the send goes out.
    expect(result.current.hasPendingSteer()).toBe(true)
    await flushMicrotasks()
    expect(result.current.hasPendingSteer()).toBe(false)
  })

  it('a steer parked mid-delivery outlives that delivery\u2019s cleanup', async () => {
    const { result, rerender, sendMessage, cancelQuery } = setup({ isQuerying: true })
    let resolveDelivery!: (ok: boolean) => void
    sendMessage.mockImplementation(() => new Promise<boolean>(res => { resolveDelivery = res }))
    act(() => {
      result.current.steer('in flight', [])
    })
    act(() => {
      rerender({ isQuerying: false })
    })
    expect(sendMessage).toHaveBeenCalledTimes(1)
    // Mid-delivery the delivered run is live again; the user steers once more.
    act(() => {
      rerender({ isQuerying: true })
    })
    act(() => {
      result.current.steer('parked behind it', [])
    })
    expect(cancelQuery).toHaveBeenCalledTimes(2)
    // The first delivery resolves — the P0-1 cleanup wiped exactly this
    // newcomer; it must still be queued and delivered on the next settle.
    await act(async () => {
      resolveDelivery(true)
    })
    expect(sendMessage).toHaveBeenCalledTimes(1)
    expect(result.current.hasPendingSteer()).toBe(true)
    act(() => {
      rerender({ isQuerying: false })
    })
    await flushMicrotasks()
    expect(sendMessage).toHaveBeenCalledTimes(2)
    expect(sendMessage).toHaveBeenNthCalledWith(2, 'parked behind it', undefined)
  })

  it('a rejected delivery does not drop the tail queued behind it', async () => {
    const { result, rerender, sendMessage, onSendRejected } = setup({ isQuerying: true })
    let resolveDelivery!: (ok: boolean) => void
    sendMessage.mockImplementation(() => new Promise<boolean>(res => { resolveDelivery = res }))
    act(() => {
      result.current.steer('head gets refused', [])
    })
    act(() => {
      result.current.steer('tail survives', [])
    })
    act(() => {
      rerender({ isQuerying: false })
    })
    await act(async () => {
      resolveDelivery(false)
    })
    expect(onSendRejected).toHaveBeenCalledTimes(1)
    expect(result.current.hasPendingSteer()).toBe(true)
    act(() => {
      rerender({ isQuerying: false })
    })
    await flushMicrotasks()
    expect(sendMessage).toHaveBeenLastCalledWith('tail survives', undefined)
  })
})

describe('useSteerSend — B1-1 timeout over the queue', () => {
  it('returns the WHOLE still-waiting batch of the visible session, in park order', () => {
    const { result, onSendRejected, sendMessage } = setup({
      isQuerying: true,
      settleTimeoutMs: 15_000,
    })
    act(() => {
      result.current.steer('batch one', [])
    })
    act(() => {
      result.current.steer('batch two', [])
    })
    act(() => {
      vi.advanceTimersByTime(14_999)
    })
    expect(onSendRejected).not.toHaveBeenCalled()
    act(() => {
      vi.advanceTimersByTime(1)
    })
    expect(onSendRejected).toHaveBeenCalledTimes(2)
    const [[pendingOne, reasonOne], [pendingTwo, reasonTwo]] = onSendRejected.mock.calls as [
      Parameters<NonNullable<SteerSendOptions['onSendRejected']>>[0],
      SteerAbortReason,
    ][]
    expect(pendingOne.text).toBe('batch one')
    expect(pendingTwo.text).toBe('batch two')
    expect(reasonOne).toBe('timeout')
    expect(reasonTwo).toBe('timeout')
    expect(sendMessage).not.toHaveBeenCalled()
    expect(result.current.hasPendingSteer()).toBe(false)
  })

  it('the timeout never reclaims the in-flight head — its send promise owns it', async () => {
    const { result, onSendRejected, rerender, sendMessage } = setup({
      isQuerying: true,
      settleTimeoutMs: 15_000,
    })
    let resolveDelivery!: (ok: boolean) => void
    sendMessage.mockImplementation(() => new Promise<boolean>(res => { resolveDelivery = res }))
    act(() => {
      result.current.steer('being delivered', [])
    })
    act(() => {
      rerender({ isQuerying: false })
    })
    act(() => {
      rerender({ isQuerying: true })
    })
    act(() => {
      result.current.steer('waiting behind it', [])
    })
    act(() => {
      vi.advanceTimersByTime(15_000)
    })
    // Only the waiting tail bounces back; the head stays out with its promise.
    expect(onSendRejected).toHaveBeenCalledTimes(1)
    const [pending, reason] = onSendRejected.mock.calls[0] as [Parameters<NonNullable<SteerSendOptions['onSendRejected']>>[0], SteerAbortReason]
    expect(pending.text).toBe('waiting behind it')
    expect(reason).toBe('timeout')
    await act(async () => {
      resolveDelivery(true)
    })
    expect(onSendRejected).toHaveBeenCalledTimes(1) // no double return
    expect(result.current.hasPendingSteer()).toBe(false)
  })

  it('timeout scoping: the visible session gives up, off-screen parks survive', async () => {
    const { result, onSendRejected, rerender, sendMessage } = setup({
      isQuerying: true,
      settleTimeoutMs: 15_000,
    })
    act(() => {
      result.current.steer('belongs to A', [])
    })
    act(() => {
      rerender({ visibleSessionId: 'sess-B', isQuerying: true })
    })
    act(() => {
      result.current.steer('belongs to B', [])
    })
    act(() => {
      vi.advanceTimersByTime(15_000)
    })
    // B is on screen — its dead wait ends; A's park is nobody's dead wait.
    expect(onSendRejected).toHaveBeenCalledTimes(1)
    const [pending] = onSendRejected.mock.calls[0] as [Parameters<NonNullable<SteerSendOptions['onSendRejected']>>[0], SteerAbortReason]
    expect(pending.text).toBe('belongs to B')
    // The gate reads the visible session only: B gave up, A's park survives
    // (proven by the delivery below) without gating anything on screen.
    expect(result.current.hasPendingSteer()).toBe(false)
    // A still delivers when its session is back and idle.
    act(() => {
      rerender({ visibleSessionId: 'sess-A', isQuerying: false })
    })
    await flushMicrotasks()
    expect(sendMessage).toHaveBeenCalledWith('belongs to A', undefined)
  })
})
