// welcomeExamples — D5 方案① 「换一批」 shuffle helper (pure).
//
// The Fisher-Yates implementation takes its random source as a parameter,
// so these tests pin the exact permutation under a stubbed source instead
// of asserting statistical properties.

import { describe, it, expect, vi, afterEach } from 'vitest'
import { WELCOME_EXAMPLES, shuffleExamples } from '@/components/welcomeExamples'

afterEach(() => {
  vi.restoreAllMocks()
})

describe('shuffleExamples', () => {
  it('produces a deterministic Fisher-Yates permutation when random always returns 0', () => {
    vi.spyOn(Math, 'random').mockReturnValue(0)
    // random() = 0 → j is always 0 → every swap pulls the head into i,
    // walking i from the tail down: [a,b,c,d] → [b,c,d,a].
    const out = shuffleExamples(['a', 'b', 'c', 'd'])
    expect(out).toEqual(['b', 'c', 'd', 'a'])
  })

  it('random always at the top of the range keeps the original order (j === i)', () => {
    vi.spyOn(Math, 'random').mockReturnValue(0.999999)
    expect(shuffleExamples(['a', 'b', 'c', 'd'])).toEqual(['a', 'b', 'c', 'd'])
  })

  it('is a permutation of the input and never mutates it', () => {
    vi.spyOn(Math, 'random').mockReturnValue(0.42)
    const deck = [...WELCOME_EXAMPLES]
    const out = shuffleExamples(WELCOME_EXAMPLES)
    expect(out).not.toBe(WELCOME_EXAMPLES)
    expect([...out].sort((a, b) => a.icon.localeCompare(b.icon))).toEqual(
      [...deck].sort((a, b) => a.icon.localeCompare(b.icon)),
    )
    expect(WELCOME_EXAMPLES).toEqual(deck)
  })

  it('the actual Math.random still yields a permutation (sanity, no stub)', () => {
    const out = shuffleExamples(WELCOME_EXAMPLES)
    expect(out).toHaveLength(WELCOME_EXAMPLES.length)
    for (const ex of WELCOME_EXAMPLES) {
      expect(out).toContainEqual(ex)
    }
  })
})
