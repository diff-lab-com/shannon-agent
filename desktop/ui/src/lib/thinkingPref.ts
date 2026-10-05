// Settings R3 T9 — 显示思考过程 (show thinking). A display-only preference
// (no engine/config surface): persistence mirrors the lib/density.ts
// localStorage pattern. Two concerns live here:
//
//   1. the three-tier pref — 'all' (default) | 'first' | 'none' — read by
//      MessageBubble (history bubbles) and StreamingResponse (live stream);
//   2. the 'first' tier's turn math — which assistant message is "the first
//      of its turn" — as a pure list projection so MessageArea's map can
//      hand each bubble a boolean (and tests can exercise the edge cases).

export type ShowThinkingPref = 'all' | 'first' | 'none'

export const SHOW_THINKING_PREF_KEY = 'shannon.chat.showThinking'

const VALID: ShowThinkingPref[] = ['all', 'first', 'none']

/** Read the persisted tier. Missing or unrecognized values fall back to
 *  'all' — the historical default (every thinking block shown, collapsed). */
export function readShowThinkingPref(): ShowThinkingPref {
  try {
    const raw = localStorage.getItem(SHOW_THINKING_PREF_KEY)
    if (raw !== null && (VALID as string[]).includes(raw)) return raw as ShowThinkingPref
  } catch { /* noop */ }
  return 'all'
}

export function setShowThinkingPref(pref: ShowThinkingPref) {
  try { localStorage.setItem(SHOW_THINKING_PREF_KEY, pref) } catch { /* noop */ }
}

/** Per-message render decision for the history list: does THIS assistant
 *  message show its thinking block under `pref`? 'none' hides everywhere;
 *  'first' only on a first-of-turn assistant (the caller computes that via
 *  firstAssistantOfTurnFlags); 'all' shows every non-empty block. */
export function shouldShowThinking(pref: ShowThinkingPref, hasThinking: boolean, isFirstOfTurn: boolean): boolean {
  if (!hasThinking) return false
  if (pref === 'none') return false
  if (pref === 'first') return isFirstOfTurn
  return true
}

/**
 * 'first' tier turn math over a whole message list. An assistant message is
 * a first-of-turn when NO assistant message has been flagged since the most
 * recent user message. Non user/assistant roles (tool / system) may
 * interleave freely: they neither start a new turn nor consume the "first"
 * slot — the flag follows the first ASSISTANT after each user message, not
 * the first message. Messages before any user message (history opening on
 * an answer) treat that first assistant as first-of-turn.
 */
export function firstAssistantOfTurnFlags(messages: { role: string }[]): boolean[] {
  let seenAssistantSinceUser = false
  return messages.map(m => {
    if (m.role === 'user') {
      seenAssistantSinceUser = false
      return false
    }
    if (m.role === 'assistant') {
      if (!seenAssistantSinceUser) {
        seenAssistantSinceUser = true
        return true
      }
      return false
    }
    return false
  })
}
