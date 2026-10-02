// R5 chat-testing plan §C — DYNAMIC accessibility scan ("find unknown
// problems" machine, part 3).
//
// walkthrough.spec.ts audits STATIC routes; this spec runs the full axe
// rule set against three chat states that only exist MID-JOURNEY, frozen
// with the same player stop points the visual matrix uses:
//
//   approval-dialog   parked on the permission alertdialog (approval-allow)
//   streaming         parked mid-stream, cursor + stop up (happy-path + pauseAt(1))
//   error-banner      mid-stream failure banner + Retry (mid-stream-fail)
//
// Gate = the walkthrough threshold: zero critical/serious violations. The
// brief's acceptance adds: CURRENT failures are recorded as known debt
// (e2e/helpers/a11yDebt.ts), not a blocker for this round — so the gate is
// enforced against KNOWN_A11Y_DEBT via matchA11yDebt, which keys every entry
// to rule + axe node target with a hard count ceiling (fix round 1/5, review
// Important 1): a new element, an extra node, or a drifted target fails even
// under an already-catalogued rule. Entries carry the reason and are removed
// when the underlying fix lands.
//
// NIGHTLY-ONLY: excluded from the PR gate by playwright.config.ts
// testIgnore; run via playwright.chat-nightly.config.ts.
import { expect, test } from '@playwright/test'
import AxeBuilder from '@axe-core/playwright'

import { ChatPage } from './helpers/ChatPage'
import { KNOWN_A11Y_DEBT, matchA11yDebt } from './helpers/a11yDebt'
import { expectMockPhase, loadChatScript } from './helpers/scriptLoader'

type ScanState = 'approval-dialog' | 'streaming' | 'error-banner'

// Review Minor 2 probe: stale-debt (fix landed → entry must be struck) is
// warn-only by default; setting A11Y_FAIL_ON_STALE_DEBT=1 on a nightly run
// turns the strike into a hard gate without touching this file.
const FAIL_ON_STALE_DEBT = process.env.A11Y_FAIL_ON_STALE_DEBT === '1'

test.describe('chat a11y — dynamic states (full axe rule set)', () => {
  /**
   * Freeze one mid-journey state, then scan it. Returns after the state's
   * anchor assertion — the caller scans whatever is on screen.
   */
  async function armState(page: import('@playwright/test').Page, state: ScanState): Promise<void> {
    switch (state) {
      case 'approval-dialog': {
        await loadChatScript(page, 'approval-allow')
        await page.getByTestId('desktop-session-row-script-sess-approval').click()
        const chat = new ChatPage(page)
        await chat.send('运行 ls -la 看看当前目录里有什么')
        await expectMockPhase(page, 'waitingPermission', 15_000)
        await expect(page.getByRole('alertdialog')).toBeVisible({ timeout: 10_000 })
        return
      }
      case 'streaming': {
        const chat = new ChatPage(page)
        await loadChatScript(page, 'happy-path')
        await page.evaluate(() => {
          (window as unknown as {
            __shannonMock?: { control: { pauseAt(i: number): void } }
          }).__shannonMock?.control.pauseAt(1)
        })
        await chat.send('帮我写一首关于海的短诗')
        await expectMockPhase(page, 'waitingUi')
        await chat.expectStreamingCursor()
        return
      }
      case 'error-banner': {
        const chat = new ChatPage(page)
        await loadChatScript(page, 'mid-stream-fail')
        await chat.send('给我讲一个关于海的故事')
        await expect(page.getByText('upstream connection reset while streaming')).toBeVisible({ timeout: 15_000 })
        await expect(page.getByRole('button', { name: 'Retry' })).toBeVisible()
        return
      }
    }
  }

  for (const state of ['approval-dialog', 'streaming', 'error-banner'] as const) {
    test(`axe full scan: ${state}`, async ({ page }) => {
      test.setTimeout(120_000)
      await armState(page, state)
      // Let the last lazy chunk / banner entrance settle before axe walks.
      await page.waitForTimeout(600)

      const results = await new AxeBuilder({ page }).analyze()
      const bad = results.violations.filter(v => v.impact === 'critical' || v.impact === 'serious')

      // Full transparency either way: attach every violation (all impacts)
      // plus the debt verdict so the nightly log/report is greppable, and
      // log the critical/serious rows.
      const verdict = matchA11yDebt(KNOWN_A11Y_DEBT, state, bad)
      await test.info().attach(`axe-${state}.json`, {
        body: JSON.stringify({
          inapplicable: results.inapplicable?.length,
          passes: results.passes?.length,
          violations: results.violations,
          debtVerdict: {
            known: verdict.known.map(k => ({ rule: k.rule, target: k.target })),
            novel: verdict.novel,
            stale: verdict.stale.map(s => ({ state: s.state, rule: s.rule })),
          },
        }, null, 2),
        contentType: 'application/json',
      })
      // eslint-disable-next-line no-console
      console.log(`[a11y] ${state}: ${results.violations.length} violation(s) total, ${bad.length} critical/serious, ${verdict.known.length} known-debt node(s), ${verdict.novel.length} novel`)

      for (const k of verdict.known) {
        // eslint-disable-next-line no-console
        console.warn(`[a11y][known-debt] ${state} / ${k.rule} @ ${JSON.stringify(k.target)} (${k.impact}): ${k.entry.reason}`)
      }
      // Stale-debt reminder: a catalogued rule that no longer violates means
      // the fix landed — strike the entry (and the report row). Warn-only by
      // default; A11Y_FAIL_ON_STALE_DEBT=1 enforces (see FAIL_ON_STALE_DEBT).
      for (const s of verdict.stale) {
        // eslint-disable-next-line no-console
        console.warn(`[a11y][stale-debt] ${state} / ${s.rule} no longer violates — remove the KNOWN_A11Y_DEBT entry: ${s.reason}`)
      }

      // The gate: any node the ledger does not explicitly absorb — a new
      // rule, a new target on an old rule, or a count above the catalogued
      // slots — fails. The message prints copy-pasteable axe targets.
      expect(
        [
          ...verdict.novel.map(v => ({ novel: v.rule, impact: v.impact, target: v.target })),
          ...(FAIL_ON_STALE_DEBT
            ? verdict.stale.map(s => ({ staleDebt: s.rule, reason: s.reason }))
            : []),
        ],
        `${state}: a11y debt gate fired\n`
          + verdict.known.map(k => `  [known-debt] ${k.rule} @ ${JSON.stringify(k.target)}: ${k.entry.reason}`).join('\n')
          + `\nRe-catalog only after human review — see e2e/helpers/a11yDebt.ts.`,
      ).toEqual([])
    })
  }
})
