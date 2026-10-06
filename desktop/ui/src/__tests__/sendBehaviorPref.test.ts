import { describe, it, expect, beforeEach } from 'vitest'
import {
  SEND_BEHAVIOR_KEY,
  readSendBehavior,
  setSendBehavior,
} from '@/lib/sendBehaviorPref'

describe('sendBehaviorPref (Settings R3 T10)', () => {
  beforeEach(() => {
    window.localStorage.clear()
  })

  describe('readSendBehavior', () => {
    it('defaults to "queue" when nothing is stored — the status quo (B1 §4-9: Enter joins the FIFO queue while streaming)', () => {
      expect(readSendBehavior()).toBe('queue')
    })

    it('reads back each persisted behavior verbatim', () => {
      for (const pref of ['steer', 'queue'] as const) {
        window.localStorage.setItem(SEND_BEHAVIOR_KEY, pref)
        expect(readSendBehavior()).toBe(pref)
      }
    })

    it('falls back to "queue" on an unrecognized stored value', () => {
      window.localStorage.setItem(SEND_BEHAVIOR_KEY, 'interject')
      expect(readSendBehavior()).toBe('queue')
    })

    it('falls back to "queue" on an empty stored value', () => {
      window.localStorage.setItem(SEND_BEHAVIOR_KEY, '')
      expect(readSendBehavior()).toBe('queue')
    })
  })

  describe('setSendBehavior', () => {
    it('persists the behavior under shannon.chat.sendBehavior', () => {
      setSendBehavior('steer')
      expect(window.localStorage.getItem(SEND_BEHAVIOR_KEY)).toBe('steer')
      setSendBehavior('queue')
      expect(window.localStorage.getItem(SEND_BEHAVIOR_KEY)).toBe('queue')
    })

    it('round-trips: the written value reads back unchanged', () => {
      setSendBehavior('steer')
      expect(readSendBehavior()).toBe('steer')
    })
  })
})
