// R2 journey #3（矩阵#3）— tool-task-file: a text lead-in, a Bash tool run
// with progress reporting, an ok tool-result, a text close and completion.
//
// Assertions: RunStatusLine visible with the elapsed pill and progress chip
// (run-status-line / run-progress-pct / run-progress-message), the tool card
// transitions running → completed form (data-tool-status), and the observed
// post-completion semantics are pinned (see the comments at those asserts —
// they double as the report's observation log for this journey).
import { expect, test } from '@playwright/test'

import { ChatPage } from './helpers/ChatPage'
import { expectMockPhase, loadChatScript, readChatScript } from './helpers/scriptLoader'
import { expectNoConsoleErrors } from './helpers/watchdog'
import type { ChatScript } from '../src/lib/mock/scripted/schema'

const script = readChatScript('tool-task-file') as ChatScript
const textChunks = script.turns[0]!.script
  .filter(s => s.event === 'query:text' && s.chunks)
  .flatMap(s => s.chunks!)
const REPLY = textChunks.join('')

test.describe('scripted chat backend — tool-task-file (journey #3)', () => {
  test('tool run surfaces the status pill, progress chip and completed card form', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'tool-task-file', test.info())

    // Open the seeded session so the run carries a real session_id (the
    // elapsed clause of the status pill reads the session's startedAt).
    await page.getByTestId('desktop-session-row-script-sess-tool').click()
    await expect(page.getByRole('heading', { name: 'Write todo.md' })).toBeVisible({ timeout: 10_000 })

    await chat.send(script.turns[0]!.user)
    await chat.expectStreamingCursor()

    // Tool starts: the live card renders in its running form.
    const runningCard = page.locator('[data-tool-name="Bash"][data-tool-status="running"]')
    await expect(runningCard).toBeVisible({ timeout: 5_000 })
    // Healthy cards render collapsed — expand to see the input summary.
    await runningCard.locator('[role="button"]').first().click()
    await expect(runningCard.getByText("printf 'buy milk\\nwrite tests\\n' > todo.md")).toBeVisible()

    // RunStatusLine appears with the elapsed pill once the run is live…
    await expect(chat.runStatusLine()).toBeVisible({ timeout: 5_000 })
    await expect(chat.runStatusLine()).toContainText('Worked')

    // …first progress event: 0.4 → 40% chip + backend message. The script
    // parks at a waitFor so this intermediate state is observable.
    await expectMockPhase(page, 'waitingUi')
    await expect(page.getByTestId('run-progress-pct')).toHaveText('· 40%')
    await expect(page.getByTestId('run-progress-message')).toHaveText('writing todo.md')
    await page.evaluate(() => {
      (window as unknown as {
        __shannonMock: { control: { resume(): void } }
      }).__shannonMock.control.resume()
    })

    // Second progress event: 0.8 → 80% + its own message.
    await expect(page.getByTestId('run-progress-pct')).toHaveText('· 80%', { timeout: 5_000 })
    await expect(page.getByTestId('run-progress-message')).toHaveText('flushing buffer')

    // ok tool-result: the card converges to the completed form.
    const doneCard = page.locator('[data-tool-name="Bash"][data-tool-status="completed"]')
    await expect(doneCard).toBeVisible({ timeout: 5_000 })
    await expect(doneCard.getByText('wrote 2 lines to todo.md')).toBeHidden() // collapsed

    // Completion: reply bubble commits, pill leaves, composer resets.
    await expect(chat.sendButton()).toBeVisible({ timeout: 15_000 })
    await expect(chat.bubbles()).toHaveCount(2)
    await chat.expectBubbleText(1, REPLY)
    await expect(chat.runStatusLine()).toHaveCount(0)

    // OBSERVED (report §journey-3): the live tool cards leave WITH the run —
    // AppContext commits the assistant message without tool_calls (P2-4:
    // cards come back only from persisted history on the next session
    // load). A scripted journey therefore never sees FileChangesCard here;
    // its +x −y rendering is pinned at component level in
    // src/__tests__/MessageBubbleFileChanges.test.tsx instead.
    await expect(page.locator('[data-tool-name="Bash"]')).toHaveCount(0)
    await expect(page.getByTestId('file-changes-card')).toHaveCount(0)
    await expectNoConsoleErrors(page)
  })
})
