// R2 journey #4（矩阵#4）— approval-allow / approval-deny: the Header's
// permission alertdialog over the ScriptedBackend's auto-parked player.
//
// The player parks itself after emitting permission-request (R1 semantics —
// the engine waits for respond_permission). Both tests assert the dialog's
// full content (tool / input / risk / reason), the respond_permission
// ledger entry via `__shannonMock.snapshot().permissionLog`, the resumption
// (tool executes / fails), and the session staying usable afterwards.
//
// Rail note: the app's main-window boot keeps currentSessionId null until a
// session row is opened, so these journeys CLICK the seeded rail row first —
// that routes the scripted session_id through the sidebar's telemetry
// (running / approval dots), which null-session journeys never reach.
import { expect, test } from '@playwright/test'

import { ChatPage } from './helpers/ChatPage'
import { expectMockPhase, loadChatScript } from './helpers/scriptLoader'
import { expectNoConsoleErrors } from './helpers/watchdog'
import { mockSnapshot } from './helpers/knownIssues'

test.describe('scripted chat backend — approval journeys (#4)', () => {
  test('Allow once records respond_permission and the tool run completes', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'approval-allow', test.info())

    // Open the seeded session so the run carries a real session_id.
    await page.getByTestId('desktop-session-row-script-sess-approval').click()
    await expect(page.getByRole('heading', { name: 'Approval flow' })).toBeVisible({ timeout: 10_000 })

    await chat.send('运行 ls -la 看看当前目录里有什么')
    // Player auto-parks after emitting the request (R1 semantics)…
    await expectMockPhase(page, 'waitingPermission', 10_000)
    // …and the Header surfaces the alertdialog.
    const dialog = page.getByRole('alertdialog')
    await expect(dialog).toBeVisible({ timeout: 5_000 })
    await expect(dialog.getByText('Permission Request')).toBeVisible()
    await expect(dialog.getByText('Review the tool invocation below')).toBeVisible()
    await expect(dialog.getByText('Bash', { exact: true })).toBeVisible()
    await expect(dialog.getByText('ls -la')).toBeVisible()
    await expect(dialog.locator('[aria-label="Risk level: High"]')).toBeVisible()
    await expect(dialog.getByText('Matched rule: shell-command')).toBeVisible()
    await expect(dialog.getByRole('button', { name: 'Deny' })).toBeVisible()
    await expect(dialog.getByRole('button', { name: 'Always Allow' })).toBeVisible()
    await expect(dialog.getByRole('button', { name: 'Allow Once' })).toBeVisible()

    // The rail's run dot marks the pending live run. While the alertdialog
    // is open, its aria-modal masking hides the rail from the accessibility
    // tree (role queries find nothing), so this mid-park assert anchors on
    // the DOM attribute — the dot is still visibly there.
    await expect(page.locator('aside [role="img"][aria-label="Running"]')).toBeVisible()

    await dialog.getByRole('button', { name: 'Allow Once' }).click()

    // respond_permission landed in the player's ledger (allow: true)…
    await expect
      .poll(async () => (await mockSnapshot(page)).permissionLog, { timeout: 5_000 })
      .toEqual([expect.objectContaining({ requestId: 'pr-allow-1', allow: true })])
    // …the dialog cleared and playback resumed into the parked tool result.
    await expect(dialog).toHaveCount(0)
    await expect(page.locator('[data-tool-name="Bash"][data-tool-status="completed"]')).toBeVisible({ timeout: 10_000 })

    // Release the pre-completion park: the run settles, composer resets.
    await page.evaluate(() => {
      (window as unknown as {
        __shannonMock: { control: { resume(): void } }
      }).__shannonMock.control.resume()
    })
    await expect(chat.sendButton()).toBeVisible({ timeout: 15_000 })
    await expect(chat.bubbles()).toHaveCount(2)

    // Rail cleared: no live dot and no amber approval dot after the run.
    await expect(page.getByRole('img', { name: 'Running' })).toHaveCount(0)
    await expect(page.getByRole('img', { name: 'Waiting for approval' })).toHaveCount(0)
    await expectNoConsoleErrors(page)
  })

  test('Deny resumes into the error-form tool card and the session stays usable', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'approval-deny', test.info())

    await page.getByTestId('desktop-session-row-script-sess-approval-deny').click()
    await expect(page.getByRole('heading', { name: 'Approval deny' })).toBeVisible({ timeout: 10_000 })

    await chat.send('删除 build 目录')
    await expectMockPhase(page, 'waitingPermission', 10_000)
    const dialog = page.getByRole('alertdialog')
    await expect(dialog).toBeVisible({ timeout: 5_000 })
    await expect(dialog.getByText('rm -rf build')).toBeVisible()
    await expect(dialog.locator('[aria-label="Risk level: Critical"]')).toBeVisible()
    await expect(dialog.getByText('Matched rule: destructive-command')).toBeVisible()

    await dialog.getByRole('button', { name: 'Deny' }).click()

    // Denial recorded; the run resumes into the parked error-form card.
    await expect
      .poll(async () => (await mockSnapshot(page)).permissionLog, { timeout: 5_000 })
      .toEqual([expect.objectContaining({ requestId: 'pr-deny-1', allow: false })])
    await expect(dialog).toHaveCount(0)
    const errorCard = page.locator('[data-tool-name="Bash"][data-tool-status="error"]')
    await expect(errorCard).toBeVisible({ timeout: 10_000 })
    // Error cards default open — the denied copy is the content.
    await expect(errorCard.getByText('Permission denied: user rejected the Bash command')).toBeVisible()

    // Release the park: the turn still settles (composer back, lead-in
    // committed) and NO session-level error banner appears (a denial is a
    // tool error, not a failed run — role=alert is the banner's anchor).
    await page.evaluate(() => {
      (window as unknown as {
        __shannonMock: { control: { resume(): void } }
      }).__shannonMock.control.resume()
    })
    await expect(chat.sendButton()).toBeVisible({ timeout: 15_000 })
    await expect(chat.bubbles()).toHaveCount(2)
    await expect(page.getByRole('alert')).toHaveCount(0)
    await expect(page.getByRole('img', { name: 'Waiting for approval' })).toHaveCount(0)
    await expectNoConsoleErrors(page)
  })

  // ── W2 G22 扩展：risk 四级配色/aria、reason 三源、legacy 兼容 ──────────

  test('risk tiers low/medium render their aria labels; llm/default/legacy reasons hit their branches', async ({ page }) => {
    test.setTimeout(90_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'approval-allow', test.info())
    await page.getByTestId('desktop-session-row-script-sess-approval').click()
    await expect(page.getByRole('heading', { name: 'Approval flow' })).toBeVisible({ timeout: 10_000 })

    const allowOnce = () => page.getByRole('alertdialog').getByRole('button', { name: 'Allow Once' })
    /** Allow + release the scripted waitFor park, so the turn settles and
     *  the composer is free for the next leg's send. */
    const allowAndSettle = async () => {
      await allowOnce().click()
      await expect(dialog).toHaveCount(0, { timeout: 5_000 })
      await page.evaluate(() => {
        (window as unknown as {
          __shannonMock: { control: { resume(): void } }
        }).__shannonMock.control.resume()
      })
      await expect(chat.sendButton()).toBeVisible({ timeout: 15_000 })
    }

    // Turn 1 (the original high/rule request) first — sends consume turns
    // in order, so the W2 legs start at the second send.
    const dialog = page.getByRole('alertdialog')
    await chat.send('运行 ls -la 看看当前目录里有什么')
    await expectMockPhase(page, 'waitingPermission', 10_000)
    await expect(dialog).toBeVisible({ timeout: 5_000 })
    await allowAndSettle()

    // Turn 2 — LOW risk + llm confidence reason (rounded to a whole %).
    await chat.send('读取 notes.txt（低风险，分类器判定）')
    await expectMockPhase(page, 'waitingPermission', 10_000)
    await expect(dialog.locator('[aria-label="Risk level: Low"]')).toBeVisible({ timeout: 5_000 })
    await expect(dialog.getByText('Safety classifier — confidence 87%')).toBeVisible()
    await allowAndSettle()

    // Turn 3 — MEDIUM risk + default-policy reason (no matched rule).
    await chat.send('写 build artifact（中风险，无匹配规则）')
    await expectMockPhase(page, 'waitingPermission', 10_000)
    await expect(dialog.locator('[aria-label="Risk level: Medium"]')).toBeVisible({ timeout: 5_000 })
    await expect(dialog.getByText('No specific rule matched — policy default')).toBeVisible()
    await allowAndSettle()

    // Turn 4 — legacy payload without a reason: no reason row at all (the
    // "Why this prompt was raised" region is absent, never a wrong label).
    await chat.send('列目录（legacy 载荷，无 reason）')
    await expectMockPhase(page, 'waitingPermission', 10_000)
    await expect(dialog.locator('[aria-label="Risk level: Low"]')).toBeVisible({ timeout: 5_000 })
    await expect(dialog.locator('[aria-label="Why this prompt was raised"]')).toHaveCount(0)
    await expect(dialog.getByText('Matched rule:')).toHaveCount(0)
    await expect(dialog.getByText('Safety classifier')).toHaveCount(0)
    await expect(dialog.getByText('No specific rule matched')).toHaveCount(0)
    await allowAndSettle()
    await expectNoConsoleErrors(page)
  })

  test('clicking the scrim backdrop denies (respond_permission allow:false recorded)', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'approval-deny', test.info())
    await page.getByTestId('desktop-session-row-script-sess-approval-deny').click()
    await expect(page.getByRole('heading', { name: 'Approval deny' })).toBeVisible({ timeout: 10_000 })

    // Turn 2's permission-request parks the player; the scrim click (the
    // fixed inset-0 backdrop BEHIND the centered panel) routes through the
    // Modal's onClose = respond Deny. It records under the FIRST turn's
    // request id — this test's single send consumes turn 0 (pr-deny-1).
    await chat.send('删除 build 目录')
    await expectMockPhase(page, 'waitingPermission', 10_000)
    const dialog = page.getByRole('alertdialog')
    await expect(dialog).toBeVisible({ timeout: 5_000 })

    await page.mouse.click(12, 400)

    // Denial recorded in the player ledger, dialog gone, run resumes into
    // the denied tool card.
    await expect
      .poll(async () => (await mockSnapshot(page)).permissionLog, { timeout: 5_000 })
      .toEqual([expect.objectContaining({ requestId: 'pr-deny-1', allow: false })])
    await expect(dialog).toHaveCount(0)
    await expect(page.locator('[data-tool-name="Bash"][data-tool-status="error"]')).toBeVisible({ timeout: 10_000 })

    await page.evaluate(() => {
      (window as unknown as {
        __shannonMock: { control: { resume(): void } }
      }).__shannonMock.control.resume()
    })
    await expect(chat.sendButton()).toBeVisible({ timeout: 15_000 })
    await expectNoConsoleErrors(page)
  })
})
