// A11y debt ledger + matcher for the dynamic a11y scan
// (chat-script.a11y.spec.ts). Extracted into a pure, import-free module so
// vitest can lock the matching semantics table-driven
// (src/__tests__/a11yDebt.test.ts) — do NOT import '@playwright/test' here.
//
// Fix round 1/5 (review Important 1): debt used to be keyed (state, ruleId)
// only, so a SECOND violator of an already-catalogued rule in the same state
// was absorbed forever — and this was not hypothetical: the very first run
// under the new matcher surfaced an uncatalogued second color-contrast node
// in approval-dialog (the header-stop-while-waiting button, 4.41:1) silently
// soaking in the badge's entry. Matching is now rule + node target with a
// hard count ceiling:
//
//   known  — a violation node absorbs one catalogued target slot of a
//            (state, rule) entry: same target, slots not exhausted.
//   novel  — a node with a target no open slot covers (new element, or a
//            same-target repeat beyond the catalogued count), or a rule with
//            no entry at all. ANY novel node fails the scan.
//   stale  — a catalogued (state, rule) whose rule no longer violates means
//            the fix landed: warned for removal, and — review Minor 2 —
//            enforced as a failure when A11Y_FAIL_ON_STALE_DEBT=1.
//
// Target drift policy: entries catalogue axe's own selector output. When the
// UI changes an element's classes, the nightly goes red with the new target
// in the failure message — re-cataloguing is a copy-paste, and that friction
// is the point (debt must track the DOM it was recorded against).

/** One segment of an axe node-target path. A string must match exactly; a
 * RegExp exists for generated ids whose suffix is mount-order dependent and
 * NOT stable across runs of the same build (observed: the Base UI permission
 * dialog portals as `#_r_l_` / `#_r_p_` — React useId-derived). Regex slots
 * are still count-bounded like every other slot. */
export type A11yTargetSegment = string | RegExp

/** An axe `node.target` selector path (element-wise through frames/shadow). */
export type A11yTarget = A11yTargetSegment[]

export interface A11yDebtEntry {
  state: string
  rule: string
  reason: string
  /** Catalogued axe node targets for this rule in this state. The entry
   * absorbs AT MOST targets.length nodes — target N consumes slot N's first
   * unused match, so a repeat or a newcomer goes novel (red). */
  targets: A11yTarget[]
}

/** Structural subset of an axe `Result` the matcher needs (kept import-free
 * of axe-core so vitest/coverage never pulls the dependency in). */
export interface A11yViolationInput {
  id: string
  impact: string | null
  nodes: Array<{ target: string[] }>
}

export interface A11yKnownNode {
  rule: string
  impact: string | null
  target: string[]
  entry: A11yDebtEntry
}

export interface A11yNovelNode {
  rule: string
  impact: string | null
  target: string[]
}

export interface A11yDebtVerdict {
  known: A11yKnownNode[]
  novel: A11yNovelNode[]
  /** Catalogued (state, rule) entries whose rule has zero violating nodes in
   * the scanned list — the fix landed, strike the entry. */
  stale: A11yDebtEntry[]
}

function segmentMatches(pattern: A11yTargetSegment, segment: string): boolean {
  return typeof pattern === 'string' ? pattern === segment : pattern.test(segment)
}

function targetMatches(pattern: A11yTarget, target: string[]): boolean {
  return pattern.length === target.length
    && pattern.every((segment, i) => segmentMatches(segment, target[i]))
}

/**
 * Split a state's critical/serious violations into known-debt absorptions,
 * novel (gate-failing) nodes, and stale (fix-landed) entries. Pure and
 * table-testable: see src/__tests__/a11yDebt.test.ts for the contract.
 */
export function matchA11yDebt(
  debt: readonly A11yDebtEntry[],
  state: string,
  badViolations: readonly A11yViolationInput[],
): A11yDebtVerdict {
  // One open slot per catalogued target; consumed greedily in declaration
  // order, so matching is deterministic.
  const slots = new Map<A11yDebtEntry, boolean[]>()
  for (const entry of debt) {
    if (entry.state === state) slots.set(entry, entry.targets.map(() => true))
  }

  const known: A11yKnownNode[] = []
  const novel: A11yNovelNode[] = []
  const violatedRules = new Set<string>()

  for (const violation of badViolations) {
    violatedRules.add(violation.id)
    for (const node of violation.nodes) {
      let absorbed = false
      for (const [entry, open] of slots) {
        if (entry.rule !== violation.id) continue
        const slot = open.findIndex((isOpen, i) => isOpen && targetMatches(entry.targets[i], node.target))
        if (slot !== -1) {
          open[slot] = false
          known.push({ rule: violation.id, impact: violation.impact, target: node.target, entry })
          absorbed = true
          break
        }
      }
      if (!absorbed) novel.push({ rule: violation.id, impact: violation.impact, target: node.target })
    }
  }

  const stale = debt.filter(entry => entry.state === state && !violatedRules.has(entry.rule))
  return { known, novel, stale }
}

/**
 * Known a11y debt (report task-5 §a11y; targets re-catalogued against real
 * axe output in fix round 1/5). NOT a silent allowlist — every scan re-runs
 * the slot matcher above, and a new element / extra node / drifted target
 * fails the nightly. Entries carry the reason and are removed when the
 * underlying fix lands (warned; A11Y_FAIL_ON_STALE_DEBT=1 enforces).
 *
 * EMPTIED 2026-10-03 (P0-A2 fix round): all four catalogued nodes were fixed
 * at the source —
 *   - aria-dialog-name (approval-dialog): the permission alertdialog is now
 *     named via aria-labelledby → its visible h3 (Header.tsx / Modal
 *     `ariaLabelledBy`).
 *   - color-contrast ×3: the sidebar live-elapsed badge uses the gated
 *     on-primary-container pair on active rows (SidebarSessions.tsx); the
 *     approval stop pill went from bg-error/80 to opaque bg-error
 *     (Header.tsx). Both ≥4.5:1 in the light and dark matrix themes.
 * The matcher stays: if a future regression needs cataloguing, re-add an
 * entry here (state + rule + real axe node targets, one slot per node) and
 * the hygiene tests in src/__tests__/a11yDebt.test.ts will hold it to shape.
 */
export const KNOWN_A11Y_DEBT: A11yDebtEntry[] = []
