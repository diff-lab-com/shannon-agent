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
// Round-1 review fixes baked in:
//   * Imp-1 — the flush effect does NOT clear the pending slot before
//     sending. It flips `deliveringRef` (a "this settle belongs to the
//     steer" token) synchronously BEFORE the drain effect runs in the same
//     commit, and the slot clears only when the delivery RESOLVES — the
//     drain can never race in and burn a queued item against the backend's
//     concurrent-query guard.
//   * Imp-2 — the pending steer is parked under its SESSION key. A settle
//     observed for a different visible session never delivers (the text
//     waits for its own session to be on screen again); a parked steer
//     whose run never settles (cancel IPC failure) is handed back to the
//     composer after `settleTimeoutMs` instead of waiting forever.

import { useCallback, useEffect, useRef } from 'react'

/** A steer message parked until its session's run settles. */
export interface SteerPending {
  /** Session the steer belongs to ('' = no session yet). */
  sessionKey: string
  text: string
  attachments: string[]
  parkedAt: number
}

/** Why a parked steer was handed back to the composer. */
export type SteerAbortReason = 'rejected' | 'timeout'

export interface SteerSendOptions {
  /** The session currently on screen (windowSessionId ?? currentSessionId). */
  visibleSessionId: string | null
  isQuerying: boolean
  cancelQuery: () => Promise<void>
  /** Chat's sendMessage. `false` = the backend rejected the send (e.g. the
   *  concurrent-query guard raced the cancel). */
  sendMessage: (text: string, attachments?: string[]) => Promise<boolean>
  /** Delivery failed or gave up — the caller restores the composer draft
   *  (`reason === 'timeout'` warrants a user-facing notice). */
  onSendRejected?: (pending: SteerPending, reason: SteerAbortReason) => void
  /** Dead-wait bound for the settle (cancel IPC failure). Default 15s. */
  settleTimeoutMs?: number
}

export interface SteerSend {
  /**
   * Accept a steer: `true` means the caller may clear the draft (the text
   * is either already sent or parked for the flush); `false` means there
   * was nothing to send. When the session is NOT streaming it degrades to
   * an ordinary send.
   */
  steer: (text: string, attachments: string[]) => boolean
  /**
   * Drain gate — true while a steer owns (or awaits) THIS visible session's
   * settle. Session-scoped: a steer parked for session A must not gate
   * session B's queue drain.
   */
  hasPendingSteer: () => boolean
}

const DEFAULT_SETTLE_TIMEOUT_MS = 15_000

export function useSteerSend({
  visibleSessionId,
  isQuerying,
  cancelQuery,
  sendMessage,
  onSendRejected,
  settleTimeoutMs = DEFAULT_SETTLE_TIMEOUT_MS,
}: SteerSendOptions): SteerSend {
  const pendingRef = useRef<SteerPending | null>(null)
  // "This settle belongs to the steer": set synchronously in the flush
  // effect (which Chat declares above its drain effect) and cleared only
  // when the delivery resolves — the drain gate reads it in the very same
  // commit the settle lands in.
  const deliveringRef = useRef(false)
  const timeoutRef = useRef<number | null>(null)
  // Latest-refs: the flush effect runs on isQuerying/visibleSession flips
  // only, so its closure must not carry stale callbacks.
  const sendMessageRef = useRef(sendMessage)
  const onSendRejectedRef = useRef(onSendRejected)
  useEffect(() => {
    sendMessageRef.current = sendMessage
    onSendRejectedRef.current = onSendRejected
  })

  // The hook re-renders whenever Chat re-renders; keep the mirror current so
  // hasPendingSteer()/timeout checks read the live visible session.
  const visibleSessionIdRef = useRef(visibleSessionId)
  visibleSessionIdRef.current = visibleSessionId

  const disarmTimeout = useCallback(() => {
    if (timeoutRef.current != null) {
      window.clearTimeout(timeoutRef.current)
      timeoutRef.current = null
    }
  }, [])

  const armTimeout = useCallback((armedFor: SteerPending) => {
    disarmTimeout()
    timeoutRef.current = window.setTimeout(() => {
      timeoutRef.current = null
      const pending = pendingRef.current
      if (!pending || pending !== armedFor) return
      if (pending.sessionKey !== (visibleSessionIdRef.current ?? '')) {
        // Parked for a session that is not on screen — not a dead wait the
        // user is watching. Keep it (it delivers on return) and re-check
        // after another window.
        armTimeout(pending)
        return
      }
      pendingRef.current = null
      onSendRejectedRef.current?.(pending, 'timeout')
    }, settleTimeoutMs)
  }, [disarmTimeout, settleTimeoutMs])

  // Give the timeout up on unmount.
  useEffect(() => disarmTimeout, [disarmTimeout])

  const steer = useCallback((text: string, attachments: string[]): boolean => {
    const trimmed = text.trim()
    const files = attachments.filter(Boolean)
    if (!trimmed && files.length === 0) return false
    if (!isQuerying) {
      // Nothing to interrupt (the run may have settled between render and
      // click): behave exactly like a normal send, restore on rejection.
      void sendMessageRef.current(trimmed, files.length > 0 ? files : undefined)
        .then(ok => {
          if (!ok) onSendRejectedRef.current?.(
            { sessionKey: visibleSessionIdRef.current ?? '', text: trimmed, attachments: files, parkedAt: Date.now() },
            'rejected',
          )
        })
      return true
    }
    void cancelQuery()
    const pending: SteerPending = {
      sessionKey: visibleSessionIdRef.current ?? '',
      text: trimmed,
      attachments: files,
      parkedAt: Date.now(),
    }
    pendingRef.current = pending
    armTimeout(pending)
    return true
  }, [isQuerying, cancelQuery, armTimeout])

  // The flush: when the interrupted run settles, deliver the parked message
  // ahead of the FIFO queue. Chat declares this hook's consumer effect above
  // its drain effect and gates the drain on hasPendingSteer — and because
  // `deliveringRef`/the pending slot stay set until the delivery RESOLVES,
  // the gate holds through the settle commit and the send itself (Imp-1).
  useEffect(() => {
    if (isQuerying || deliveringRef.current) return
    const pending = pendingRef.current
    if (!pending) return
    // Imp-2: only the steer's OWN session may receive it — a settle observed
    // while another session is on screen keeps the text parked (it delivers
    // when its session comes back into view and is idle).
    if (pending.sessionKey !== (visibleSessionIdRef.current ?? '')) return
    deliveringRef.current = true
    disarmTimeout()
    void sendMessageRef.current(pending.text, pending.attachments.length > 0 ? pending.attachments : undefined)
      .then(ok => {
        deliveringRef.current = false
        pendingRef.current = null
        if (!ok) onSendRejectedRef.current?.(pending, 'rejected')
      })
      .catch(() => {
        deliveringRef.current = false
        pendingRef.current = null
        onSendRejectedRef.current?.(pending, 'rejected')
      })
  }, [isQuerying, visibleSessionId, disarmTimeout])

  const hasPendingSteer = useCallback(() => {
    if (deliveringRef.current) return true
    const pending = pendingRef.current
    return pending != null && pending.sessionKey === (visibleSessionIdRef.current ?? '')
  }, [])

  return { steer, hasPendingSteer }
}
