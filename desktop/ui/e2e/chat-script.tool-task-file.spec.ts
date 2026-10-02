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

  // ── W2 G19 扩展：FileCard 动作面（四选四做全，取舍见各注释） ────────────
  // 1. PDF 预览懒加载 + 失败态（read_attachment 演示不可用 = 失败 fixture）；
  // 2. csv Batch run → Build prompt 推 composer 草稿（永不自动发送）；
  // 3. save-as 取消静默无害（plugin:dialog|save → null，无 toast 无报错）；
  // 4. reveal/open 失败 toast（OS 表面演示不可用 → toastError）。
  // 取舍说明：「抽取明细」（extraction 报告区）需要 seed 附件携带 extraction
  // 元数据的 schema 再扩一字段，本 wave 从简不做——见报告 G19 节。
  test('FileCard action surface: pdf preview fails honest, csv builds a draft, save-as cancel is silent, reveal/open toast', async ({ page }) => {
    test.setTimeout(90_000)
    await loadChatScript(page, 'tool-task-file', test.info())
    await page.getByTestId('desktop-session-row-script-sess-files').click()
    await expect(page.getByRole('heading', { name: 'Generated files' })).toBeVisible({ timeout: 10_000 })

    // Three generated files render as FileCards under their tool blocks.
    const pdfCard = page.getByTestId('file-card').filter({ hasText: 'report.pdf' })
    const csvCard = page.getByTestId('file-card').filter({ hasText: 'data.csv' })
    const mdCard = page.getByTestId('file-card').filter({ hasText: 'notes.md' })
    await expect(pdfCard).toBeVisible({ timeout: 10_000 })
    await expect(csvCard).toBeVisible()
    await expect(mdCard).toBeVisible()

    // 1. PDF preview: the ~1MB pdf.js chunk is LAZY — the preview only
    // mounts on click. read_attachment is OS-surface unmocked → the honest
    // failed state inside the dialog (never a hang, never a crash).
    await pdfCard.getByRole('button', { name: 'Preview' }).click()
    await expect(page.getByTestId('pdf-preview')).toBeVisible({ timeout: 15_000 })
    await expect(page.getByTestId('pdf-preview-failed')).toBeVisible({ timeout: 15_000 })
    await page.keyboard.press('Escape')

    // 2. csv Batch run: the dialog builds a per-row instruction draft —
    // pushed into the composer via shannon:composer-draft, NEVER auto-sent.
    await csvCard.getByTestId('file-card-batch-run').click()
    await page.getByTestId('batch-instruction-input').fill('把每行的数量翻倍')
    await page.getByTestId('batch-build-prompt').click()
    await expect(page.getByTestId('batch-instruction-input')).toHaveCount(0)
    await expect(page.getByRole('textbox', { name: 'Message' })).toContainText('data.csv', { timeout: 10_000 })
    await expect(page.getByRole('button', { name: 'Send message' })).toBeVisible()
    await expect(page.locator('[data-message-index]')).toHaveCount(2) // seeded only — nothing auto-sent

    // 3. Save as → the demo dialog resolves the CANCEL shape (null): the
    // card backs out silently — no success toast, no error, console clean.
    await mdCard.getByRole('button', { name: 'Save as…' }).click()
    await expect(page.getByText(/Saved to/)).toHaveCount(0, { timeout: 5_000 })

    // 4. reveal / open: OS surfaces stay unmocked → the failure toast is the
    // honest outcome. Their console noise is this test's declared fixture.
    const osNoise = [
      /unhandled Tauri command: "reveal_in_folder"/,
      /unhandled Tauri command: "open_with_default_app"/,
      /unhandled Tauri command: "read_attachment"/,
    ]
    await mdCard.getByRole('button', { name: 'Show in folder' }).click()
    await expect(page.getByText('Failed to open link').first()).toBeVisible({ timeout: 10_000 })
    await pdfCard.getByRole('button', { name: 'Open attachment' }).click()
    await expect(page.getByText('Failed to open link').first()).toBeVisible({ timeout: 10_000 })

    await expectNoConsoleErrors(page, osNoise)
  })
})
