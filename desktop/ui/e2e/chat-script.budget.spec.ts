// R3 journey #7（矩阵#7）— budget-exceeded: the seeded over-budget session
// (spentUsd 6.4 ≥ budgetUsd 5) shows the red exceeded banner via the
// mount/switch re-derivation (B4 P2-8), the budget-cap auto-cancel settles
// the run, and "Continue (ignore once)" resends with the budget-bypass flag.
//
// Finding anchors: A-2 (the bypass resend drops the original attachments —
// asserted below via the player's sends log) and the R2 walkthrough's
// budget-continuation finding.
import { expect, test } from '@playwright/test'

import { ChatPage } from './helpers/ChatPage'
import { loadChatScript, readChatScript } from './helpers/scriptLoader'
import { expectNoConsoleErrors } from './helpers/watchdog'
import { annotateKnownIssues, mockSnapshot } from './helpers/knownIssues'
import type { ChatScript } from '../src/lib/mock/scripted/schema'

const script = readChatScript('budget-exceeded') as ChatScript

test.describe('scripted chat backend — budget-exceeded (journey #7)', () => {
  test('exceeded banner three actions, auto-cancel at the cap, Continue once rides budgetBypass', async ({ page }) => {
    test.setTimeout(60_000)
    annotateKnownIssues(test.info(), {
      'A-2': 'The budget "Continue once" resend drops the last user message\'s attachments '
        + '(Chat.tsx continuePastBudget resends content only). Current behavior is asserted below '
        + '(sends[1].attachments === null); flip to "attachments preserved" when R4 lands — the '
        + 'seeded user message carries /Users/demo/Downloads/report-draft.md.',
    })
    const chat = new ChatPage(page)
    await loadChatScript(page, 'budget-exceeded', test.info())

    // Open the seeded session: the banner re-derives from the persisted pair
    // (get_session_budget 5 / get_session_usage 6.4) without any event.
    await page.getByTestId('desktop-session-row-script-sess-budget').click()
    await expect(page.getByRole('heading', { name: 'Over budget' })).toBeVisible({ timeout: 10_000 })

    // Red exceeded banner with the frozen three actions. VARIANT ANCHOR
    // (CI fix): the $-body text alone is ambiguous — budget.warning.body and
    // budget.exceeded.body share the "{spent} of {budget} used" prefix
    // (en.json), so a banner mid-flip between the re-derive
    // (useBudgetGuard.ts:52-76) and a budget:* event could satisfy the old
    // hasText pair while the buttons (exceeded-only, BudgetBanner.tsx:56-88)
    // were not yet up. 'Choose how to proceed.' is exceeded-only; each
    // action assert carries its own 15s window so a slow CI runner rides
    // out the variant settle instead of inheriting a 5s default mid-flip.
    // Finding anchor (provider review §3-A1): the ApiKeyBanner now shows
    // ONLY on a genuine missing-key/missing-provider snapshot — the armed
    // seed's hasKey:true keeps it absent here (the filtered alert queries
    // below would catch an extra alert); the four-quadrant gating itself is
    // pinned by src/__tests__/ApiKeyBanner.test.tsx (R2).
    const banner = page.getByRole('alert').filter({ hasText: 'Choose how to proceed' })
    await expect(banner).toBeVisible({ timeout: 15_000 })
    await expect(banner.getByText(/\$6\.40 of \$5\.00 used/)).toBeVisible({ timeout: 15_000 })
    await expect(banner.getByRole('button', { name: 'Continue (ignore once)' })).toBeVisible({ timeout: 15_000 })
    await expect(banner.getByRole('button', { name: 'Raise budget…' })).toBeVisible({ timeout: 15_000 })
    await expect(banner.getByRole('button', { name: 'Stop', exact: true })).toBeVisible({ timeout: 15_000 })

    // "Raise budget…" opens the budget dialog (the second action is alive).
    await banner.getByRole('button', { name: 'Raise budget…' }).click()
    await expect(page.getByRole('dialog').getByText('Set session budget')).toBeVisible()
    await page.getByRole('dialog').getByRole('button', { name: 'Cancel' }).click()

    // The turn: budget:exceeded mid-stream → the cap auto-cancels (same
    // token as Stop) — cancelled settles, no assistant bubble, no error.
    await chat.send(script.turns[0]!.user)
    // Wide mid-run window (3 × 800ms chunks) — the full suite runs workers
    // in parallel and the first ticks after a send can be slow.
    await expect(page.locator('.streaming-cursor')).toBeVisible({ timeout: 15_000 })
    await expect(chat.sendButton()).toBeVisible({ timeout: 15_000 })
    // Seeded history (2) + the new user bubble; cancelled commits no reply.
    await expect(chat.bubbles()).toHaveCount(3)
    await expect(page.getByRole('img', { name: 'Last run failed' })).toHaveCount(0)

    // Continue once: clearExceeded + resend with the bypass flag — snapshot
    // proves the wire arg, and A-2's dropped attachment.
    await banner.getByRole('button', { name: 'Continue (ignore once)' }).click()
    await expect(banner).toHaveCount(0)
    const snapshot = await mockSnapshot(page)
    expect(snapshot.sends[1]).toMatchObject({
      turnIndex: 1,
      budgetBypass: true,
      attachments: null, // A-2 current behavior — flip with the fix
      sessionId: 'script-sess-budget',
    })
    // The bypass turn streams to completion.
    await expect(chat.bubbles()).toHaveCount(5, { timeout: 15_000 })
    await expect(chat.bubbleAt(4)).toContainText('数据来源已补充')
    await expectNoConsoleErrors(page)
  })
})
