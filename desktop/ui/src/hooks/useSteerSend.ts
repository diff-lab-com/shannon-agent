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
// B1-1 (P0-1): parked steers live in a per-session FIFO, not a single
// global slot. A lone slot let a steer for session A be silently
// overwritten by one for session B while A's settle was still awaited —
// and the text was already gone from the composer, so it was lost without
// a trace; a park landing while a delivery was in flight was likewise
// wiped by that delivery's cleanup. Queues only build up when a user fires
// several interrupts before their runs settle — exactly then losing one is
// least excusable, so every entry survives and goes out in park order.
//
// Round-1 review fixes baked in:
//   * Imp-1 — the flush effect does NOT release the queue before sending.
//     It flips `deliveringRef` (a "this settle belongs to the steer" token
//     carrying the delivered session key) synchronously BEFORE the drain
//     effect runs in the same commit, and the entry leaves the queue only
//     when the delivery RESOLVES — the drain can never race in and burn a
//     queued item against the backend's concurrent-query guard.
//   * Imp-2 — steers are parked under their SESSION key. A settle observed
//     for a different visible session never delivers (the text waits for
//     its own session to be on screen again); a parked steer whose run
//     never settles (cancel IPC failure) is handed back to the composer
//     after `settleTimeoutMs` instead of waiting forever.

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
   *  (`reason === 'timeout'` warrants a user-facing notice; a timeout may
   *  fire for several queued entries one after another, so append rather
   *  than overwrite). */
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
  // Per-session FIFO of steers waiting for their session's settle. Entries
  // are parked under the key they were steered from and never move, so
  // reading the VISIBLE key's queue head is the Imp-2 session check itself.
  // Entries leave by object reference (delivery resolution, timeout abort)
  // — never wholesale, so a park landing mid-delivery always outlives it.
  const pendingRef = useRef<Map<string, SteerPending[]>>(new Map())
  // "This settle belongs to the steer": the key whose head is being
  // delivered right now, or null. Set synchronously in the flush effect
  // (which Chat declares above its drain effect) and cleared only when the
  // delivery resolves — the drain gate reads it in the very same commit the
  // settle lands in. Single-flight by construction: the flush only starts
  // when this is null.
  const deliveringRef = useRef<string | null>(null)
  const timeoutRef = useRef<number | null>(null)
  // Latest-refs: the flush effect runs on every render, so its closure must
  // not carry stale callbacks.
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

  // Remove one entry, by reference, from its session's queue. The queue
  // itself stays when it still has tail entries — clearing it wholesale is
  // the P0-1 loss this hook exists to prevent.
  const dropPending = useCallback((pending: SteerPending) => {
    const queue = pendingRef.current.get(pending.sessionKey)
    if (!queue) return
    const at = queue.indexOf(pending)
    if (at >= 0) queue.splice(at, 1)
    if (queue.length === 0) pendingRef.current.delete(pending.sessionKey)
  }, [])

  const disarmTimeout = useCallback(() => {
    if (timeoutRef.current != null) {
      window.clearTimeout(timeoutRef.current)
      timeoutRef.current = null
    }
  }, [])

  // (Re)arm the dead-wait bound. Re-armed on every park, so the window
  // counts from the latest park. On expiry only the session the user is
  // WATCHING gives up: every entry of its queue still waiting for the
  // settle is returned in park order. The in-flight head (if any) is not
  // waiting — its own send promise owns its fate now (the flush always
  // delivers queue[0] and removes it by reference, so the head is exactly
  // queue[0] while `deliveringRef` holds this key). Off-screen parks are
  // not a dead wait anyone is watching: they stay parked (delivering on
  // return) and re-arm to keep checking.
  const armTimeout = useCallback(() => {
    disarmTimeout()
    timeoutRef.current = window.setTimeout(() => {
      timeoutRef.current = null
      const visible = visibleSessionIdRef.current ?? ''
      let offscreenWaiting = false
      for (const [key, queue] of pendingRef.current) {
        if (queue.length === 0) continue
        if (key !== visible) {
          offscreenWaiting = true
          continue
        }
        const delivering = deliveringRef.current === key
        for (const pending of delivering ? queue.slice(1) : queue) {
          onSendRejectedRef.current?.(pending, 'timeout')
        }
        if (delivering) queue.length = 1
        else pendingRef.current.delete(key)
      }
      if (offscreenWaiting) armTimeout()
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
    // Queue tail: a steer parked while this session's earlier steer is
    // still parked (or being delivered) must outlive it, not replace it.
    let queue = pendingRef.current.get(pending.sessionKey)
    if (!queue) {
      queue = []
      pendingRef.current.set(pending.sessionKey, queue)
    }
    queue.push(pending)
    armTimeout()
    return true
  }, [isQuerying, cancelQuery, armTimeout])

  // The flush: when the interrupted run settles, deliver the queued steers
  // ahead of the FIFO prompt queue, head first. Chat declares this hook's
  // consumer effect above its drain effect and gates the drain on
  // hasPendingSteer — and because `deliveringRef`/the head stay in place
  // until the delivery RESOLVES, the gate holds through the settle commit
  // and the send itself (Imp-1). Runs on every render (no dep array): the
  // gates below make the extra runs no-ops, and a tail parked behind a
  // finished delivery is picked up by the next commit even when no
  // isQuerying/visibility flip follows.
  useEffect(() => {
    if (isQuerying || deliveringRef.current != null) return
    const key = visibleSessionIdRef.current ?? ''
    const queue = pendingRef.current.get(key)
    const pending = queue?.[0]
    if (!pending) return
    deliveringRef.current = key
    disarmTimeout()
    void sendMessageRef.current(pending.text, pending.attachments.length > 0 ? pending.attachments : undefined)
      .then(ok => {
        deliveringRef.current = null
        dropPending(pending)
        if (!ok) onSendRejectedRef.current?.(pending, 'rejected')
      })
      .catch(() => {
        deliveringRef.current = null
        dropPending(pending)
        onSendRejectedRef.current?.(pending, 'rejected')
      })
  })

  const hasPendingSteer = useCallback(() => {
    const key = visibleSessionIdRef.current ?? ''
    if (deliveringRef.current === key) return true
    const queue = pendingRef.current.get(key)
    return queue != null && queue.length > 0
  }, [])

  return { steer, hasPendingSteer }
}
