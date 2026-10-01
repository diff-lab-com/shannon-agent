// R3 journey #10（矩阵#10）— edit-rewind: the seeded 2-turn history derives
// per-turn checkpoints (armed list_checkpoints), the edit banner + composer
// prefill appear, the commit rewinds the session (armed rewind_session
// truncates to the checkpoint boundary) and streams the edited resend;
// cancelling the edit restores the pre-edit draft. The second test doubles
// as the A-14 boundary-evidence run (checkpoint turn_index == the edited
// message's turn is still rewindable).
import { expect, test } from '@playwright/test'

import { ChatPage } from './helpers/ChatPage'
import { loadChatScript, readChatScript } from './helpers/scriptLoader'
import { expectNoConsoleErrors } from './helpers/watchdog'
import type { ChatScript } from '../src/lib/mock/scripted/schema'

const script = readChatScript('edit-rewind') as ChatScript
const EDITED = script.turns[0]!.user

async function openSeededSession(page: import('@playwright/test').Page): Promise<ChatPage> {
  const chat = new ChatPage(page)
  await page.getByTestId('desktop-session-row-script-sess-edit').click()
  await expect(page.getByRole('heading', { name: 'Edit and rewind' })).toBeVisible({ timeout: 10_000 })
  await expect(chat.bubbles()).toHaveCount(4)
  return chat
}

/** Hover a user bubble to reveal its hover toolbar, then click Edit. */
async function startEdit(page: import('@playwright/test').Page, index: number): Promise<void> {
  await page.locator(`[data-message-index="${index}"]`).hover()
  await page.getByRole('button', { name: 'Edit message' }).first().click()
  await expect(page.getByTestId('edit-banner')).toBeVisible()
}

test.describe('scripted chat backend — edit-rewind (journey #10)', () => {
  test('edit banner → commit rewinds the session and streams the edited resend', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'edit-rewind', test.info())
    await openSeededSession(page)

    // Edit the FIRST user message (checkpoint at its own turn — A-14
    // boundary equality, rewindable).
    await startEdit(page, 0)
    await expect(chat.composer()).toHaveValue('第一轮：先介绍潮汐的成因')

    // Commit: rewind_session truncates the seeded conversation to before
    // the edited turn (turn 0 → empty), then the resend streams.
    await chat.composer().fill(EDITED)
    await chat.composer().press('Enter')
    await expect(page.getByTestId('edit-banner')).toHaveCount(0)
    // The rewind truncated everything; the resend appends user + reply.
    await expect(chat.bubbles()).toHaveCount(2, { timeout: 20_000 })
    await chat.expectBubbleText(0, EDITED)
    await chat.expectBubbleText(1, '编辑后的回答：潮汐按周期可分为半日潮、全日潮和混合潮。')
    await expectNoConsoleErrors(page)
  })

  test('cancel (Escape) exits the edit and restores the pre-edit draft (A-14 boundary evidence)', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'edit-rewind', test.info())
    await openSeededSession(page)

    // A-14 EVIDENCE (recorded in the report, no behavior change): message
    // index 2 owns turn 1 and the derived checkpoint turn_index === 1 —
    // the Edit button is still offered (rewindInfoFor's `>=` treats exact
    // equality as rewindable).
    await startEdit(page, 2)
    await expect(chat.composer()).toHaveValue('第二轮：再讲讲风暴潮')

    // Escape tier 1: exit the edit — no query is cancelled (idle session),
    // the pre-edit draft (empty here) comes back.
    await chat.composer().press('Escape')
    await expect(page.getByTestId('edit-banner')).toHaveCount(0)
    await expect(chat.composer()).toHaveValue('')
    await expect(chat.bubbles()).toHaveCount(4)
    await expectNoConsoleErrors(page)
  })
})
