// PM-12: feedback keys must be stable across reloads and sensitive to content.
import { describe, it, expect } from 'vitest'
import { hash32, messageFeedbackKey } from '@/lib/feedbackKey'

describe('messageFeedbackKey', () => {
  it('is deterministic for the same input', () => {
    expect(messageFeedbackKey(1700000000, 'hello')).toBe(messageFeedbackKey(1700000000, 'hello'))
  })

  it('differs for different timestamps or content', () => {
    const base = messageFeedbackKey(1700000000, 'hello')
    expect(base).not.toBe(messageFeedbackKey(1700000001, 'hello'))
    expect(base).not.toBe(messageFeedbackKey(1700000000, 'hellp'))
  })

  it('hash32 is a stable 32-bit unsigned value', () => {
    expect(hash32('')).toBe(0x811c9dc5)
    const h = hash32('shannon')
    expect(h).toBeGreaterThan(0)
    expect(h).toBeLessThanOrEqual(0xffffffff)
  })
})
