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

    // Red exceeded banner with the frozen three actions (scoping: other
    // role=alert regions can coexist on the page).
    const banner = page.getByRole('alert').filter({ hasText: 'Session budget exceeded' })
    await expect(banner).toBeVisible({ timeout: 10_000 })
    await expect(banner.getByText(/\$6\.40 of \$5\.00 used/)).toBeVisible()
    await expect(banner.getByRole('button', { name: 'Continue (ignore once)' })).toBeVisible()
    await expect(banner.getByRole('button', { name: 'Raise budget…' })).toBeVisible()
    await expect(banner.getByRole('button', { name: 'Stop', exact: true })).toBeVisible()

    // "Raise budget…" opens the budget dialog (the second action is alive).
    await banner.getByRole('button', { name: 'Raise budget…' }).click()
    await expect(page.getByRole('dialog').getByText('Set session budget')).toBeVisible()
    await page.getByRole('dialog').getByRole('button', { name: 'Cancel' }).click()

    // The turn: budget:exceeded mid-stream → the cap auto-cancels (same
    // token as Stop) — cancelled settles, no assistant bubble, no error.
    await chat.send(script.turns[0]!.user)
    await chat.expectStreamingCursor()
    await expect(chat.sendButton()).toBeVisible({ timeout: 15_000 })
    await expect(chat.bubbles()).toHaveCount(1) // only the user bubble
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
    await expect(chat.bubbles()).toHaveCount(3, { timeout: 15_000 })
    await expect(chat.bubbleAt(2)).toContainText('数据来源已补充')
    await expectNoConsoleErrors(page)
  })
})
