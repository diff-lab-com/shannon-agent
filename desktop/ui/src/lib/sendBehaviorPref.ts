// Settings R3 T10 — send-while-running behavior. A pure front-end flow
// preference (no engine/config surface): persistence mirrors the
// lib/thinkingPref.ts localStorage pattern.
//
// Status-quo survey (why the default is 'queue'): Chat.handleSend — the
// Enter / send-button path — has always joined the session's FIFO prompt
// queue while a run streams (B1 §4-9). The dedicated interrupt affordance
// (composer bolt button / Ctrl+Enter → Chat.handleSteer) is the explicit
// steer and keeps steering in BOTH modes; the pref re-routes only the
// plain send.

export type SendBehavior = 'steer' | 'queue'

export const SEND_BEHAVIOR_KEY = 'shannon.chat.sendBehavior'

const VALID: SendBehavior[] = ['steer', 'queue']

/** Read the persisted behavior. Missing or unrecognized values fall back to
 *  'queue' — the historical default (a send while this session streams
 *  joins its FIFO queue). */
export function readSendBehavior(): SendBehavior {
  try {
    const raw = localStorage.getItem(SEND_BEHAVIOR_KEY)
    if (raw !== null && (VALID as string[]).includes(raw)) return raw as SendBehavior
  } catch { /* noop */ }
  return 'queue'
}

export function setSendBehavior(pref: SendBehavior) {
  try { localStorage.setItem(SEND_BEHAVIOR_KEY, pref) } catch { /* noop */ }
}
