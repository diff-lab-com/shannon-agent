// R3 journey #14（矩阵#14）— context-panels: /context and /cost render the
// SlashResultCard (diagnostics pinned above the composer, dismissible) and
// the composer's usage dialog shows the seeded spend; the mid-run
// query:usage event is the usageTick that makes the dialog's breakdown
// card refetch (asserted L1-side; here the dialog values are pinned).
import { expect, test } from '@playwright/test'

import { ChatPage } from './helpers/ChatPage'
import { expectMockPhase, loadChatScript, readChatScript } from './helpers/scriptLoader'
import { expectNoConsoleErrors } from './helpers/watchdog'
import type { ChatScript } from '../src/lib/mock/scripted/schema'

const script = readChatScript('context-panels') as ChatScript

test.describe('scripted chat backend — context-panels (journey #14)', () => {
  test('slash cards for /context and /cost, then the usage dialog with the seeded spend', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'context-panels', test.info())
    await page.getByTestId('desktop-session-row-script-sess-panels').click()
    await expect(page.getByRole('heading', { name: 'Panels' })).toBeVisible({ timeout: 10_000 })

    // /context — the demo context stats projection.
    await chat.send('/context')
    const contextCard = page.getByText('Context usage').first()
    await expect(contextCard).toBeVisible({ timeout: 10_000 })
    await expect(page.getByText('4,820')).toBeVisible()
    await page.getByRole('button', { name: 'Dismiss' }).click()

    // /cost — the seeded spentUsd answers get_session_usage. The card's
    // currency formatter uses the locale default (2 fraction digits).
    await chat.send('/cost')
    const costCard = page.getByText('Session cost').first()
    await expect(costCard).toBeVisible({ timeout: 10_000 })
    await expect(page.getByText('$0.07', { exact: true })).toBeVisible()
    await page.getByRole('button', { name: 'Dismiss' }).click()

    // The turn parks after its query:usage event; open the usage dialog —
    // AppContext.usage (the tick the dialog receives) already updated.
    await chat.send(script.turns[0]!.user)
    await expectMockPhase(page, 'waitingUi')
    await page.getByRole('button', { name: 'Session usage' }).click()
    const dialog = page.getByRole('dialog')
    await expect(dialog.getByText('Context composition')).toBeVisible({ timeout: 10_000 })
    // The summary line reads the seeded ledger pair (spent / cap-less →
    // label) — the same get_session_usage the /cost card showed.
    await expect(dialog.getByText('$0.0731')).toBeVisible()
    await dialog.getByRole('button', { name: 'Cancel' }).click()

    await page.evaluate(() => {
      (window as unknown as { __shannonMock: { control: { resume(): void } } }).__shannonMock.control.resume()
    })
    await expect(chat.sendButton()).toBeVisible({ timeout: 15_000 })
    await expectNoConsoleErrors(page)
  })
})
