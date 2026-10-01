// useSteerSend — GB P2-10a: the "interrupt now" half of two-tier steering.
//
// While THIS session streams, plain sends join its FIFO queue (B1 §4-9,
// unchanged). The other tier is Cursor-style immediate interjection: the
// user cancels the running turn and their message goes out the moment the
// run settles — BEFORE any queued prompts drain. The engine has no
// inject-mid-run channel, so "interrupt" is composed from two existing
// primitives: cancelQuery() + a send that waits for the QUERY_CANCELLED
// settle (the isQuerying flip), which is exactly the escape hatch the
// Escape key always offered — with the draft preserved instead of dropped.
//
// The flush must win the race against Chat's queue-drain effect (both fire
// on the same isQuerying→false commit): Chat gates its drain on
// `hasPendingSteer()`, and this hook's effect is declared first, so an
// interrupted turn's message lands ahead of the queue.

import { useCallback, useEffect, useRef } from 'react'

/** A steer message parked until its session's run settles. */
export interface SteerPending {
  text: string
  attachments: string[]
}

export interface SteerSendOptions {
  isQuerying: boolean
  cancelQuery: () => Promise<void>
  /** Chat's sendMessage. `false` = the backend rejected the send (e.g. the
   *  concurrent-query guard raced the cancel). */
  sendMessage: (text: string, attachments?: string[]) => Promise<boolean>
  /** Delivery failed — the caller restores the composer draft from it. */
  onSendRejected?: (pending: SteerPending) => void
}

export interface SteerSend {
  /**
   * Accept a steer: `true` means the caller may clear the draft (the text
   * is either already sent or parked for the flush); `false` means there
   * was nothing to send. When the session is NOT streaming it degrades to
   * an ordinary send.
   */
  steer: (text: string, attachments: string[]) => boolean
  /** Drain-gate: true while an interrupted steer awaits its flush. */
  hasPendingSteer: () => boolean
}

export function useSteerSend({
  isQuerying,
  cancelQuery,
  sendMessage,
  onSendRejected,
}: SteerSendOptions): SteerSend {
  const pendingRef = useRef<SteerPending | null>(null)
  // Latest-refs: the flush effect runs on isQuerying flips only, so its
  // closure must not carry a stale sendMessage/onSendRejected from the
  // run's first render.
  const sendMessageRef = useRef(sendMessage)
  const onSendRejectedRef = useRef(onSendRejected)
  useEffect(() => {
    sendMessageRef.current = sendMessage
    onSendRejectedRef.current = onSendRejected
  })

  const steer = useCallback((text: string, attachments: string[]): boolean => {
    const trimmed = text.trim()
    const files = attachments.filter(Boolean)
    if (!trimmed && files.length === 0) return false
    if (!isQuerying) {
      // Nothing to interrupt (the run may have settled between render and
      // click): behave exactly like a normal send, restore on rejection.
      void sendMessageRef.current(trimmed, files.length > 0 ? files : undefined)
        .then(ok => {
          if (!ok) onSendRejectedRef.current?.({ text: trimmed, attachments: files })
        })
      return true
    }
    void cancelQuery()
    pendingRef.current = { text: trimmed, attachments: files }
    return true
  }, [isQuerying, cancelQuery])

  // The flush: when the interrupted run settles, deliver the parked message
  // ahead of the FIFO queue (Chat declares this hook's consumer effect above
  // its drain effect and gates the drain on hasPendingSteer). A rejected
  // delivery hands the text back instead of burning it.
  useEffect(() => {
    if (isQuerying) return
    const pending = pendingRef.current
    if (!pending) return
    pendingRef.current = null
    void sendMessageRef.current(pending.text, pending.attachments.length > 0 ? pending.attachments : undefined)
      .then(ok => {
        if (!ok) onSendRejectedRef.current?.(pending)
      })
  }, [isQuerying])

  const hasPendingSteer = useCallback(() => pendingRef.current != null, [])

  return { steer, hasPendingSteer }
}
