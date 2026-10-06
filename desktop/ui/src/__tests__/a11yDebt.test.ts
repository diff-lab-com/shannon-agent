// Fix round 1/5 (review Important 1): locks the a11y debt matcher's
// discrimination contract. The old (state, ruleId) matcher absorbed any
// second violator of a catalogued rule forever; matchA11yDebt must absorb
// ONLY rule + exact node target, bounded by the catalogued slot count, and
// must flag fix-landed entries as stale. The real ledger KNOWN_A11Y_DEBT is
// asserted for hygiene so a malformed entry (empty targets, duplicate
// state+rule) fails here instead of silently weakening the nightly gate.
//
// P0-A2 (2026-10-03): the ledger was EMPTIED — all four catalogued nodes
// were fixed at the source (see a11yDebt.ts). The suite is now
// semantics-preserving: the matcher contract tests below run entirely on
// local fixtures and keep matchA11yDebt locked so FUTURE debt can still be
// catalogued safely, and the ledger-hygiene block guards whatever entries
// (if any) exist at that time — with an empty ledger it passes vacuously,
// which is exactly the clean-baseline contract.
import { describe, expect, it } from 'vitest'

import { KNOWN_A11Y_DEBT, matchA11yDebt, type A11yDebtEntry, type A11yViolationInput } from '../../e2e/helpers/a11yDebt'

/** Table row: one fake critical violation with N nodes at the given targets. */
function v(rule: string, targets: string[][]): A11yViolationInput {
  return { id: rule, impact: 'serious', nodes: targets.map(target => ({ target })) }
}

const ENTRY: A11yDebtEntry = {
  state: 'approval-dialog',
  rule: 'color-contrast',
  reason: 'test entry',
  targets: [
    ['.badge'],
    ['.stop-button'],
  ],
}

describe('matchA11yDebt — slot semantics', () => {
  it('absorbs a node whose target equals a catalogued slot (same state + rule)', () => {
    const { known, novel } = matchA11yDebt([ENTRY], 'approval-dialog', [v('color-contrast', [['.badge']])])
    expect(novel).toEqual([])
    expect(known).toHaveLength(1)
    expect(known[0]).toMatchObject({ rule: 'color-contrast', target: ['.badge'], entry: ENTRY })
  })

  it('absorbs each node against its own slot (multi-node violation, count == slots)', () => {
    const { known, novel } = matchA11yDebt(
      [ENTRY],
      'approval-dialog',
      [v('color-contrast', [['.badge'], ['.stop-button']])],
    )
    expect(novel).toEqual([])
    expect(known.map(k => k.target)).toEqual([['.badge'], ['.stop-button']])
  })

  it('fails a NEW target under an already-catalogued rule (the reviewed hole)', () => {
    const { known, novel } = matchA11yDebt([ENTRY], 'approval-dialog', [v('color-contrast', [['.brand-new-widget']])])
    expect(known).toEqual([])
    expect(novel).toEqual([{ rule: 'color-contrast', impact: 'serious', target: ['.brand-new-widget'] }])
  })

  it('fails a same-target repeat beyond the catalogued slot count', () => {
    const { known, novel } = matchA11yDebt(
      [ENTRY],
      'approval-dialog',
      [v('color-contrast', [['.badge'], ['.badge']])],
    )
    expect(known).toHaveLength(1)
    expect(novel).toEqual([{ rule: 'color-contrast', impact: 'serious', target: ['.badge'] }])
  })

  it('fails a rule with no entry in the ledger at all', () => {
    const { known, novel } = matchA11yDebt([ENTRY], 'approval-dialog', [v('label', [['.thing']])])
    expect(known).toEqual([])
    expect(novel).toEqual([{ rule: 'label', impact: 'serious', target: ['.thing'] }])
  })

  it('never absorbs across states', () => {
    const { known, novel } = matchA11yDebt([ENTRY], 'streaming', [v('color-contrast', [['.badge']])])
    expect(known).toEqual([])
    expect(novel).toHaveLength(1)
  })

  it('never absorbs across rules sharing a target', () => {
    const other: A11yDebtEntry = { ...ENTRY, rule: 'aria-dialog-name' }
    const { known, novel } = matchA11yDebt([other], 'approval-dialog', [v('color-contrast', [['.badge']])])
    expect(known).toEqual([])
    expect(novel).toHaveLength(1)
  })

  it('fails when the selector path differs (length or a deeper segment)', () => {
    const framed: A11yDebtEntry = { ...ENTRY, targets: [['iframe', '.badge']] }
    expect(matchA11yDebt([framed], 'approval-dialog', [v('color-contrast', [['.badge']])]).novel).toHaveLength(1)
    expect(matchA11yDebt([framed], 'approval-dialog', [v('color-contrast', [['iframe', '.other']])]).novel).toHaveLength(1)
    expect(matchA11yDebt([framed], 'approval-dialog', [v('color-contrast', [['iframe', '.badge']])]).novel).toEqual([])
  })
})

describe('matchA11yDebt — RegExp segments (generated ids)', () => {
  const dialog: A11yDebtEntry = {
    state: 'approval-dialog',
    rule: 'aria-dialog-name',
    reason: 'test entry',
    targets: [[/^#_r_[0-9a-z_]+$/i]],
  }

  it('absorbs a generated id matching the pattern, still count-bounded', () => {
    const first = matchA11yDebt([dialog], 'approval-dialog', [v('aria-dialog-name', [['#_r_l_']])])
    expect(first.novel).toEqual([])
    // A different mount-order suffix on a SECOND dialog exceeds the one slot.
    const second = matchA11yDebt(
      [dialog],
      'approval-dialog',
      [v('aria-dialog-name', [['#_r_l_'], ['#_r_p_']])],
    )
    expect(second.known).toHaveLength(1)
    expect(second.novel).toEqual([{ rule: 'aria-dialog-name', impact: 'serious', target: ['#_r_p_'] }])
  })

  it('does not let the pattern swallow unrelated ids', () => {
    const { novel } = matchA11yDebt([dialog], 'approval-dialog', [v('aria-dialog-name', [['#sidebar']])])
    expect(novel).toHaveLength(1)
  })
})

describe('matchA11yDebt — stale detection (Minor 2)', () => {
  it('flags a catalogued rule that no longer violates (fix landed)', () => {
    const { stale } = matchA11yDebt([ENTRY], 'approval-dialog', [v('label', [['.thing']])])
    expect(stale).toEqual([ENTRY])
  })

  it('does not flag while the rule still violates, even partly unmatched', () => {
    const { stale } = matchA11yDebt([ENTRY], 'approval-dialog', [v('color-contrast', [['.other']])])
    expect(stale).toEqual([])
  })

  it('is scoped to the scanned state', () => {
    expect(matchA11yDebt([ENTRY], 'streaming', []).stale).toEqual([])
  })
})

describe('KNOWN_A11Y_DEBT — ledger hygiene', () => {
  it('is a well-formed ledger: every entry has catalogued slots, no duplicate state+rule', () => {
    // Vacuously true while the ledger stays empty (P0-A2 baseline); the
    // invariants hold the line the day debt is re-catalogued.
    const seen = new Set<string>()
    for (const entry of KNOWN_A11Y_DEBT) {
      expect(entry.targets.length, `${entry.state}/${entry.rule} must catalogue >=1 target`).toBeGreaterThan(0)
      for (const target of entry.targets) {
        expect(target.length, `${entry.state}/${entry.rule} target path must be non-empty`).toBeGreaterThan(0)
      }
      const key = `${entry.state}/${entry.rule}`
      expect(seen.has(key), `duplicate ledger entry ${key}`).toBe(false)
      seen.add(key)
    }
  })

  it('string catalogued slots self-absorb when the recorded scan is replayed', () => {
    // Guards against an entry being hand-edited into a form that can never
    // absorb its own scan output (e.g. a typo'd selector), which would
    // perma-red the nightly. RegExp segments (generated ids) are covered by
    // the pattern tests above and by the nightly run itself.
    for (const entry of KNOWN_A11Y_DEBT.filter(e => e.targets.every(t => t.every(s => typeof s === 'string')))) {
      const replay = matchA11yDebt(
        KNOWN_A11Y_DEBT,
        entry.state,
        entry.targets.map(target => v(entry.rule, [target as string[]])),
      )
      for (const target of entry.targets) {
        expect(
          replay.known.some(k => k.rule === entry.rule && JSON.stringify(k.target) === JSON.stringify(target)),
          `${entry.state}/${entry.rule} slot ${JSON.stringify(target)} must self-absorb`,
        ).toBe(true)
      }
    }
  })

  it('an empty ledger is a pure gate: every violation is novel, nothing is stale', () => {
    // Pins the P0-A2 clean-baseline semantics: with no entries the nightly
    // fails on ANY critical/serious node and stale-debt can never fire.
    const { known, novel, stale } = matchA11yDebt(KNOWN_A11Y_DEBT, 'approval-dialog', [
      v('color-contrast', [['.anything']]),
    ])
    expect(KNOWN_A11Y_DEBT).toEqual([])
    expect(known).toEqual([])
    expect(novel).toHaveLength(1)
    expect(stale).toEqual([])
  })
})
