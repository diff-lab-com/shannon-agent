// R3 journey #10（矩阵#10）— edit-rewind: the seeded 2-turn history derives
// per-turn checkpoints (armed list_checkpoints), the edit banner + composer
// prefill appear, the commit rewinds the session (armed rewind_session
// truncates to the checkpoint boundary) and streams the edited resend;
// cancelling the edit restores the pre-edit draft. The second test doubles
// as the A-14 boundary-evidence run (checkpoint turn_index == the edited
// message's turn is still rewindable).
//
// wave-2 quickwin: the first seeded user message carries two attachments —
// the A-26 test pins WYSIWYG editing (chips load from the message on
// startEdit; remove/add during the edit is exactly what the resend carries,
// asserted through the player's sends log).
import { expect, test } from '@playwright/test'

import { ChatPage } from './helpers/ChatPage'
import { loadChatScript, readChatScript } from './helpers/scriptLoader'
import { expectNoConsoleErrors } from './helpers/watchdog'
import { mockSnapshot } from './helpers/knownIssues'
import { emitWebviewDrop } from './helpers/webviewDrop'
import type { ChatScript } from '../src/lib/mock/scripted/schema'

const script = readChatScript('edit-rewind') as ChatScript
const EDITED = script.turns[0]!.user
// The A-26 commit's edited text: each quickwin test is a FRESH script
// lifecycle whose single send always consumes turn 0, so the turn list
// stays at one entry and this test-local text never maps to a yaml turn.
const EDITED_2 = '第一轮（编辑后，附件已调整）：潮汐的成因和周期'

const NOTES_PATH = script.seed!.sessions![0]!.messages[0]!.attachments![0]!
const CHART_PATH = script.seed!.sessions![0]!.messages[0]!.attachments![1]!
const NOTES_NAME = NOTES_PATH.split('/').pop()!
const CHART_NAME = CHART_PATH.split('/').pop()!
// The added-during-edit file (inside the demo working dir → no refusal badge).
const ADDED_PATH = '/Users/demo/workspace/my-startup/storm-surge.md'
const ADDED_NAME = ADDED_PATH.split('/').pop()!

async function openSeededSession(page: import('@playwright/test').Page): Promise<ChatPage> {
  const chat = new ChatPage(page)
  await page.getByTestId('desktop-session-row-script-sess-edit').click()
  await expect(page.getByRole('heading', { name: 'Edit and rewind' })).toBeVisible({ timeout: 10_000 })
  await expect(chat.bubbles()).toHaveCount(4)
  return chat
}

/** Hover a user bubble to reveal its hover toolbar, then click Edit. */
async function startEdit(page: import('@playwright/test').Page, index: number): Promise<void> {
  const bubble = page.locator(`[data-message-index="${index}"]`)
  await bubble.hover()
  // Scope to the bubble — every user message carries an Edit button.
  await bubble.getByRole('button', { name: 'Edit message' }).click()
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
    // User bubbles carry chrome (You · time + actions) — assert containment.
    await expect(chat.bubbleAt(0)).toContainText(EDITED)
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

  test('A-26 WYSIWYG: edit chips load from the message; remove/add during the edit is what the resend carries', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'edit-rewind', test.info())
    await openSeededSession(page)

    await startEdit(page, 0)
    // WYSIWYG load: the seeded message's own attachments appear as composer
    // chips (anchored on their remove buttons — the bubbles' FileCards show
    // the same names, so the buttons are the unambiguous chip anchor).
    await expect(page.getByRole('button', { name: `Remove ${NOTES_NAME}` })).toBeVisible()
    await expect(page.getByRole('button', { name: `Remove ${CHART_NAME}` })).toBeVisible()

    // Edit the set: drop tide-notes.md, add storm-surge.md.
    await page.getByRole('button', { name: `Remove ${NOTES_NAME}` }).click()
    await expect(page.getByRole('button', { name: `Remove ${NOTES_NAME}` })).toHaveCount(0)
    await emitWebviewDrop(page, [ADDED_PATH])
    await expect(page.getByRole('button', { name: `Remove ${ADDED_NAME}` })).toBeVisible()
    await expect(page.getByRole('button', { name: `Remove ${CHART_NAME}` })).toBeVisible()

    // Commit: the resend streams with the EDITED attachment set (the reply
    // bubble is turn 0's scripted text — each test owns a fresh lifecycle).
    await chat.composer().fill(EDITED_2)
    await chat.composer().press('Enter')
    await expect(page.getByTestId('edit-banner')).toHaveCount(0)
    await expect(chat.bubbles()).toHaveCount(2, { timeout: 20_000 })
    await expect(chat.bubbleAt(0)).toContainText(EDITED_2)

    // Payload truth (player sends log): the composer's current set travels —
    // NOT the message's original pair the old editing.attachmentPaths commit
    // silently resurrected.
    const sends = (await mockSnapshot(page)).sends
    expect(sends).toHaveLength(1)
    expect(sends[0]).toMatchObject({ message: EDITED_2, attachments: [CHART_PATH, ADDED_PATH] })
    await expectNoConsoleErrors(page)
  })

  test('A-25: clearing the text during an edit and pressing Enter resends attachments-only (no silent return)', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'edit-rewind', test.info())
    await openSeededSession(page)

    await startEdit(page, 0)
    // WYSIWYG chips are loaded (A-26); emptying the text leaves an
    // attachments-only composer — the A-25 gate must treat Enter as a
    // commit, not a silent return.
    await expect(page.getByRole('button', { name: `Remove ${NOTES_NAME}` })).toBeVisible()
    await chat.composer().fill('')
    await chat.composer().press('Enter')

    // NOT silent: the edit exits and the attachments-only resend streams.
    await expect(page.getByTestId('edit-banner')).toHaveCount(0)
    await expect(chat.bubbles()).toHaveCount(2, { timeout: 20_000 })
    // The optimistic user bubble carries the attachment previews (A-4).
    await expect(chat.bubbleAt(0)).toContainText(NOTES_NAME)
    await expect(chat.bubbleAt(0)).toContainText(CHART_NAME)

    // Payload truth: empty text + the loaded chip set (untouched → the
    // message's original pair).
    const sends = (await mockSnapshot(page)).sends
    expect(sends).toHaveLength(1)
    expect(sends[0]).toMatchObject({ message: '', attachments: [NOTES_PATH, CHART_PATH] })
    await expectNoConsoleErrors(page)
  })
})
