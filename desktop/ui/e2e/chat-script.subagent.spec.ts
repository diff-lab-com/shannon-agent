// R3 journey #12（矩阵#12）— subagent-run: the agent_spawn card renders as a
// first-class SubagentBlock; subagent:start lights the "registry <id>" live
// badge while it runs; the stop event + tool-result converge the block,
// which then leaves with the run (P2-4).
import { expect, test } from '@playwright/test'

import { ChatPage } from './helpers/ChatPage'
import { expectMockPhase, loadChatScript, readChatScript } from './helpers/scriptLoader'
import { expectNoConsoleErrors } from './helpers/watchdog'
import type { ChatScript } from '../src/lib/mock/scripted/schema'

const script = readChatScript('subagent-run') as ChatScript

test.describe('scripted chat backend — subagent-run (journey #12)', () => {
  test('SubagentBlock appears live with the registry badge and converges after subagent:stop', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'subagent-run', test.info())
    await page.getByTestId('desktop-session-row-script-sess-subagent').click()
    await expect(page.getByRole('heading', { name: 'Subagent run' })).toBeVisible({ timeout: 10_000 })

    await chat.send(script.turns[0]!.user)
    await chat.expectStreamingCursor()

    // The spawn renders as its own block (not a generic tool card), in the
    // running form with the registry badge from subagent:start.
    const block = page.getByTestId('subagent-block')
    await expect(block).toBeVisible({ timeout: 5_000 })
    await expect(block).toContainText('Subagent · researcher')
    await expect(block).toContainText('registry sa-research-1')
    await expect(page.locator('[data-tool-name="agent_spawn"]')).toHaveCount(0) // never a generic card

    // Expanded: the spawn prompt fields + the parent approval-mode note.
    await block.getByRole('button').first().click()
    await expect(block.getByText('Max turns: 8')).toBeVisible()
    await expect(block.getByTestId('subagent-inherit-mode')).toBeVisible()
    await block.getByRole('button').first().click() // collapse again

    // The script parks at its waitFor while the spawn is live; resume into
    // tool-result + subagent:stop (which parks again).
    await expectMockPhase(page, 'waitingUi')
    await page.evaluate(() => {
      (window as unknown as { __shannonMock: { control: { resume(): void } } }).__shannonMock.control.resume()
    })

    // Convergence: the live registry badge clears and the block settles
    // into the completed form (client-measured duration label; the running
    // spinner is gone) — observable thanks to the second park.
    await expect(block).not.toContainText('registry sa-research-1', { timeout: 10_000 })
    await expect(block.locator('.animate-spin')).toHaveCount(0)

    // Final resume → completion: the card leaves with the run (P2-4); the
    // reply commits.
    await expectMockPhase(page, 'waitingUi')
    await page.evaluate(() => {
      (window as unknown as { __shannonMock: { control: { resume(): void } } }).__shannonMock.control.resume()
    })
    await expect(block).toHaveCount(0, { timeout: 15_000 })
    await expect(chat.sendButton()).toBeVisible({ timeout: 15_000 })
    await expect(chat.bubbles()).toHaveCount(2)
    await expectNoConsoleErrors(page)
  })
})
