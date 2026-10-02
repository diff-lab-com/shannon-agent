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
// (below), not a blocker for this round — so the gate is enforced against
// KNOWN_A11Y_DEBT: anything already catalogued (state + rule id) is
// reported and attached, anything NEW fails. Entries carry the reason and
// are removed when the underlying fix lands.
//
// NIGHTLY-ONLY: excluded from the PR gate by playwright.config.ts
// testIgnore; run via playwright.chat-nightly.config.ts.
import { expect, test } from '@playwright/test'
import AxeBuilder from '@axe-core/playwright'

import { ChatPage } from './helpers/ChatPage'
import { expectMockPhase, loadChatScript } from './helpers/scriptLoader'

type ScanState = 'approval-dialog' | 'streaming' | 'error-banner'

/**
 * Known a11y debt (report task-5, §a11y): critical/serious violations
 * present at scan time, keyed by state + rule. NOT a silent allowlist —
 * every scan re-attaches the full violation report and the R5 report
 * enumerates them. A NEW rule (or a new state hitting an old rule) fails.
 */
const KNOWN_A11Y_DEBT: Array<{ state: ScanState, rule: string, reason: string }> = [
  {
    state: 'approval-dialog',
    rule: 'aria-dialog-name',
    reason: 'Header.tsx permission Modal (role="alertdialog") carries no accessible name — the visible h3 title is not wired via aria-labelledby / aria-label. serious; fix = label the dialog (business component, out of R5 scope).',
  },
  {
    state: 'approval-dialog',
    rule: 'color-contrast',
    reason: 'SidebarSessions.tsx live-elapsed badge (font-mono text-label-2xs text-secondary) fails 4.5:1 on the rail surface while the run is live. serious; only rendered mid-run — the static walkthrough never sees it.',
  },
  {
    state: 'streaming',
    rule: 'color-contrast',
    reason: 'Same SidebarSessions live-elapsed badge as approval-dialog (the row runs live during the parked stream). serious.',
  },
]

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
      // and log the critical/serious rows so the nightly log is greppable.
      await test.info().attach(`axe-${state}.json`, {
        body: JSON.stringify({ inapplicable: results.inapplicable?.length, passes: results.passes?.length, violations: results.violations }, null, 2),
        contentType: 'application/json',
      })
      // eslint-disable-next-line no-console
      console.log(`[a11y] ${state}: ${results.violations.length} violation(s) total, ${bad.length} critical/serious`)

      const debtFor = (rule: string) => KNOWN_A11Y_DEBT.find(d => d.state === state && d.rule === rule)
      const known = bad.filter(v => debtFor(v.id) != null)
      const novel = bad.filter(v => debtFor(v.id) == null)
      for (const v of known) {
        // eslint-disable-next-line no-console
        console.warn(`[a11y][known-debt] ${state} / ${v.id} (${v.impact}, ${v.nodes.length} nodes): ${debtFor(v.id)?.reason}`)
      }
      // Stale-debt reminder: a catalogued rule that no longer violates means
      // the fix landed — strike the entry (and the report row).
      const badIds = new Set(bad.map(v => v.id))
      for (const entry of KNOWN_A11Y_DEBT.filter(d => d.state === state)) {
        if (!badIds.has(entry.rule)) {
          // eslint-disable-next-line no-console
          console.warn(`[a11y][stale-debt] ${state} / ${entry.rule} no longer violates — remove the KNOWN_A11Y_DEBT entry: ${entry.reason}`)
        }
      }
      expect(
        novel.map(v => ({ id: v.id, impact: v.impact, nodes: v.nodes.slice(0, 3).map(n => n.target) })),
        `${state}: NEW critical/serious violations (not in KNOWN_A11Y_DEBT)\n`
          + known.map(v => `  [known-debt] ${v.id}: ${debtFor(v.id)?.reason}`).join('\n'),
      ).toEqual([])
    })
  }
})
