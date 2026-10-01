// GB P2-10a — the interrupt-now steering hook.
//
// Contract pins:
//   1. steer() while streaming parks the message and cancels the run.
//   2. The parked message flushes the moment isQuerying flips false —
//      ahead of anything else (Chat gates its drain on hasPendingSteer).
//   3. A rejected flush hands the text back (nothing is burned).
//   4. Idle steer degrades to an ordinary send.
//   5. Empty steer is a no-op (caller keeps the draft).

import { describe, it, expect, vi } from 'vitest'
import { renderHook, act, waitFor } from '@testing-library/react'
import { useSteerSend, type SteerSendOptions } from '@/hooks/useSteerSend'

function setup(overrides: Partial<SteerSendOptions> = {}) {
  const cancelQuery = vi.fn().mockResolvedValue(undefined)
  const sendMessage = vi.fn().mockResolvedValue(true)
  const onSendRejected = vi.fn()
  const utils = renderHook((props: Partial<SteerSendOptions> = {}) =>
    useSteerSend({
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

describe('useSteerSend', () => {
  it('while streaming: parks the message and cancels the run', () => {
    const { result, cancelQuery, sendMessage } = setup({ isQuerying: true })
    let accepted = false
    act(() => {
      accepted = result.current.steer('stop that, use Rust instead', [])
    })
    expect(accepted).toBe(true)
    expect(cancelQuery).toHaveBeenCalledTimes(1)
    expect(result.current.hasPendingSteer()).toBe(true)
    // Nothing sent yet — the run must settle first.
    expect(sendMessage).not.toHaveBeenCalled()
  })

  it('flushes the parked message when the run settles, with attachments', async () => {
    const { result, rerender, sendMessage } = setup({ isQuerying: true })
    act(() => {
      result.current.steer('now run the tests', ['/repo/a.diff'])
    })
    act(() => {
      rerender({ isQuerying: false })
    })
    await waitFor(() =>
      expect(sendMessage).toHaveBeenCalledWith('now run the tests', ['/repo/a.diff']),
    )
    expect(result.current.hasPendingSteer()).toBe(false)
  })

  it('a rejected flush restores the draft via onSendRejected (nothing burned)', async () => {
    const { result, rerender, sendMessage, onSendRejected } = setup({
      isQuerying: true,
    })
    sendMessage.mockResolvedValue(false)
    act(() => {
      result.current.steer('interrupt text', [])
    })
    act(() => {
      rerender({ isQuerying: false })
    })
    await waitFor(() => expect(onSendRejected).toHaveBeenCalledWith({ text: 'interrupt text', attachments: [] }))
  })

  it('idle steer degrades to an ordinary send (with restore on rejection)', async () => {
    const { result, cancelQuery, sendMessage, onSendRejected } = setup({ isQuerying: false })
    let accepted = true
    act(() => {
      accepted = result.current.steer('normal message', [])
    })
    expect(accepted).toBe(true)
    expect(cancelQuery).not.toHaveBeenCalled()
    await waitFor(() => expect(sendMessage).toHaveBeenCalledWith('normal message', undefined))
    expect(onSendRejected).not.toHaveBeenCalled()
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
})
