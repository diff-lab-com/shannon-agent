// R3 journey #8（矩阵#8）— attachments: the draft-restored attachment chips,
// the P0-3 rejection receipt on send (one "«file» was not sent: «reason»"
// toast per refused path — the R2 walkthrough's out-of-working-dir P0
// finding anchor), and the optimistic bubble's attachment preview (A-4
// fixed in R4 group 1).
import { expect, test } from '@playwright/test'

import { ChatPage } from './helpers/ChatPage'
import { loadChatScript, readChatScript } from './helpers/scriptLoader'
import { expectNoConsoleErrors } from './helpers/watchdog'
import type { ChatScript } from '../src/lib/mock/scripted/schema'

const script = readChatScript('attachments') as ChatScript
const OUTSIDE_PATH = script.turns[0]!.rejectedAttachments![0]!.path
const OUTSIDE_NAME = OUTSIDE_PATH.split('/').pop()!
const DRAFT_KEY = 'shannon.draft.script-sess-attach'

test.describe('scripted chat backend — attachments (journey #8)', () => {
  test('draft chips render, the send returns a rejection receipt toast, the optimistic bubble carries the attachment (A-4 fixed)', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    // The draft (text + out-of-working-dir attachment) rides localStorage
    // before the app boots — the same key Chat.tsx's per-session drafts use.
    await page.addInitScript(([key, path]) => {
      localStorage.setItem(key, JSON.stringify({ text: '总结这个文件', attachments: [path], updatedAt: Date.now() }))
    }, [DRAFT_KEY, OUTSIDE_PATH] as const)
    await loadChatScript(page, 'attachments', test.info())
    // The Chat page must be mounted BEFORE the row click: the draft-restore
    // effect keys on the visible-session CHANGE, and a click landing before
    // the page's prev-ref initialized would skip the restore entirely.
    await expect(chat.composer()).toBeVisible({ timeout: 10_000 })

    // Opening the session restores the draft: chip + text in the composer.
    await page.getByTestId('desktop-session-row-script-sess-attach').click()
    await expect(page.getByRole('heading', { name: 'Attachments' })).toBeVisible({ timeout: 10_000 })
    // The chip anchors on its remove button (stable aria-label with the name).
    const chipRemove = page.getByRole('button', { name: `Remove ${OUTSIDE_NAME}` })
    await expect(chipRemove).toBeVisible({ timeout: 5_000 })
    const chip = page.locator('span', { has: chipRemove })
    await expect(chip.getByText(OUTSIDE_NAME)).toBeVisible()
    await expect(chat.composer()).toHaveValue('总结这个文件')

    // OBSERVED (report §journey-8): the restored chip carries no warning
    // badge — the badge comes from the attach-time preflight
    // (check_attachment_paths), which a draft restore bypasses (native file
    // dialog not drivable in the harness). The send-time refusal below is
    // the P0 finding's assertion surface.
    await expect(chip.getByTestId('attachment-chip-issue')).toHaveCount(0)

    await chat.send('总结这个文件')

    // P0-3 receipt: the refusal surfaces as its own toast — never a silent
    // drop. (Regression anchor: attachments outside the working directory.)
    const toast = page.locator('[data-sonner-toast]').filter({ hasText: OUTSIDE_NAME })
    await expect(toast).toBeVisible({ timeout: 10_000 })
    await expect(toast).toContainText('was not sent')
    await expect(toast).toContainText('outside the working directory')

    // A-4 fixed: the optimistic user bubble carries its attachment in the
    // backend ChatMessage's wire shape — the FileCard renders the filename
    // immediately, no reload needed.
    await expect(chat.bubbleAt(0)).toBeVisible()
    await expect(chat.bubbleAt(0)).toContainText(OUTSIDE_NAME)
    // Accepted sends clear the draft + chips.
    await expect(chat.composer()).toHaveValue('')
    await expect(chipRemove).toHaveCount(0)

    // The turn still completes (partial success) and the reply commits.
    await expect(chat.bubbles()).toHaveCount(2, { timeout: 15_000 })
    await expect(chat.bubbleAt(1)).toContainText('工作目录之外')
    await expectNoConsoleErrors(page)
  })
})
