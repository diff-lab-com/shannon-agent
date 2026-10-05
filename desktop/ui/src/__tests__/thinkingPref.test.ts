import { describe, it, expect, beforeEach } from 'vitest'
import {
  SHOW_THINKING_PREF_KEY,
  readShowThinkingPref,
  setShowThinkingPref,
  shouldShowThinking,
  firstAssistantOfTurnFlags,
} from '@/lib/thinkingPref'

describe('thinkingPref (Settings R3 T9)', () => {
  beforeEach(() => {
    window.localStorage.clear()
  })

  describe('readShowThinkingPref', () => {
    it('defaults to "all" when nothing is stored', () => {
      expect(readShowThinkingPref()).toBe('all')
    })

    it('reads back each persisted tier verbatim', () => {
      for (const pref of ['all', 'first', 'none'] as const) {
        window.localStorage.setItem(SHOW_THINKING_PREF_KEY, pref)
        expect(readShowThinkingPref()).toBe(pref)
      }
    })

    it('falls back to "all" on an unrecognized stored value', () => {
      window.localStorage.setItem(SHOW_THINKING_PREF_KEY, 'sometimes')
      expect(readShowThinkingPref()).toBe('all')
    })

    it('falls back to "all" on an empty stored value', () => {
      window.localStorage.setItem(SHOW_THINKING_PREF_KEY, '')
      expect(readShowThinkingPref()).toBe('all')
    })
  })

  describe('setShowThinkingPref', () => {
    it('persists the tier under shannon.chat.showThinking', () => {
      setShowThinkingPref('first')
      expect(window.localStorage.getItem(SHOW_THINKING_PREF_KEY)).toBe('first')
      expect(readShowThinkingPref()).toBe('first')
      setShowThinkingPref('none')
      expect(readShowThinkingPref()).toBe('none')
    })
  })

  describe('shouldShowThinking', () => {
    it('"all" shows every non-empty thinking block', () => {
      expect(shouldShowThinking('all', true, false)).toBe(true)
      expect(shouldShowThinking('all', true, true)).toBe(true)
    })

    it('"first" only shows a first-of-turn assistant', () => {
      expect(shouldShowThinking('first', true, true)).toBe(true)
      expect(shouldShowThinking('first', true, false)).toBe(false)
    })

    it('"none" hides everywhere', () => {
      expect(shouldShowThinking('none', true, true)).toBe(false)
      expect(shouldShowThinking('none', true, false)).toBe(false)
    })

    it('never shows when the message carries no thinking', () => {
      expect(shouldShowThinking('all', false, true)).toBe(false)
      expect(shouldShowThinking('first', false, true)).toBe(false)
      expect(shouldShowThinking('none', false, false)).toBe(false)
    })
  })

  describe('firstAssistantOfTurnFlags', () => {
    it('flags the first assistant after each user message', () => {
      const flags = firstAssistantOfTurnFlags([
        { role: 'user' },
        { role: 'assistant' },
        { role: 'user' },
        { role: 'assistant' },
      ])
      expect(flags).toEqual([false, true, false, true])
    })

    it('flags only the first of consecutive assistant messages', () => {
      const flags = firstAssistantOfTurnFlags([
        { role: 'user' },
        { role: 'assistant' },
        { role: 'assistant' },
        { role: 'assistant' },
      ])
      expect(flags).toEqual([false, true, false, false])
    })

    it('returns empty for an empty list', () => {
      expect(firstAssistantOfTurnFlags([])).toEqual([])
    })

    it('interleaved tool/system messages neither start a turn nor consume the first slot', () => {
      const flags = firstAssistantOfTurnFlags([
        { role: 'user' },
        { role: 'tool' },
        { role: 'assistant' },
        { role: 'tool' },
        { role: 'assistant' },
        { role: 'system' },
        { role: 'user' },
        { role: 'tool' },
        { role: 'assistant' },
      ])
      // Index 3 is the first ASSISTANT after the user at 0; the earlier tool
      // message does not take the slot. After the user at 6, the assistant
      // at 8 is first again.
      expect(flags).toEqual([false, false, true, false, false, false, false, false, true])
    })

    it('treats an assistant opening the list (no preceding user) as first-of-turn', () => {
      const flags = firstAssistantOfTurnFlags([
        { role: 'assistant' },
        { role: 'assistant' },
        { role: 'user' },
        { role: 'assistant' },
      ])
      expect(flags).toEqual([true, false, false, true])
    })
  })
})
