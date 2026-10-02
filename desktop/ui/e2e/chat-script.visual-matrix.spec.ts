// R5 chat-testing plan §6 — visual state MATRIX for the chat page
// ("find unknown problems" machine, part 2): 6 conversation states ×
// light+dark = 12 toHaveScreenshot baselines (maxDiffPixelRatio 0.05, the
// visual-baseline.spec.ts headroom).
//
// State manufacturing reuses the R1/R2 pattern: loadChatScript + the
// player's UI stop points (`waitFor: 'ui'` in the script, or the
// control.pauseAt equivalent) so the intermediate states are FROZEN, not
// racing the screenshot:
//
//   welcome        seeded empty session, nothing sent            (happy-path)
//   streaming      parked after the text chunks — the reply sits
//                  in the streaming bucket, cursor + stop up     (happy-path + pauseAt(1))
//   tool-running   parked on a running Bash card + 40% pill      (visual-tool-run)
//   approval       parked on the permission alertdialog          (approval-allow)
//   error-banner   mid-stream failure banner + Retry             (mid-stream-fail)
//   settled        full run committed — user + reply bubbles     (happy-path)
//
// Stability contract (why this can run in CI at 0.05):
//   - animations/transitions/caret are killed via a style tag (the
//     themes.spec technique — the streaming cursor pulse and the running
//     spinner otherwise blur every frame);
//   - wall-clock content is masked, not asserted away: bubble <time> stamps
//     and the elapsed-time run-status pill ("worked for Xs", a 1s tick).
//
// NIGHTLY-ONLY: excluded from the PR gate by playwright.config.ts
// testIgnore; run via playwright.chat-nightly.config.ts. Re-baseline after
// an intentional visual change:
//   pnpm exec playwright test e2e/chat-script.visual-matrix.spec.ts --update-snapshots
//   (then commit the 12 new PNGs — baselines are append/replace-per-file,
//   never wholesale-deleted).
import { expect, test } from '@playwright/test'

import { ChatPage } from './helpers/ChatPage'
import { expectMockPhase, loadChatScript } from './helpers/scriptLoader'

const THEMES = [
  { mode: 'light', id: 'material' },
  { mode: 'dark', id: 'tokyo-night' },
] as const

const STOP_ANIMATIONS = '*, *::before, *::after { animation: none !important; transition: none !important; caret-color: transparent !important; }'

/** Mask the wall-clock chrome: bubble timestamps + the elapsed-time pill. */
const DYNAMIC = [
  () => 'time',
  () => '[data-testid="run-status-line"]',
]

test.describe('chat visual state matrix (light × dark)', () => {
  for (const theme of THEMES) {
    test.describe(`theme: ${theme.mode}`, () => {
      // Every test in this file needs the theme armed BEFORE the app boots —
      // the themes.spec mechanism (localStorage via addInitScript).
      test.beforeEach(async ({ page }) => {
        await page.addInitScript(id => {
          window.localStorage.setItem('shannon-theme', id as string)
        }, theme.id)
      })

      test(`welcome state [${theme.mode}]`, async ({ page }) => {
        await loadChatScript(page, 'happy-path')
        await expect(page.getByRole('heading', { name: 'What can I help with?' })).toBeVisible({ timeout: 15_000 })
        await expect(page.getByRole('textbox', { name: 'Message' })).toBeVisible()
        // Theme actually landed before the pixels are read (themes.spec).
        await expect(page.locator('html')).toHaveAttribute('data-theme-mode', theme.mode)
        await page.addStyleTag({ content: STOP_ANIMATIONS })
        await page.waitForTimeout(400)
        await expect(page).toHaveScreenshot(`chat-matrix-welcome-${theme.mode}.png`, {
          maxDiffPixelRatio: 0.05,
          mask: DYNAMIC.map(f => page.locator(f())),
        })
      })

      test(`streaming state [${theme.mode}]`, async ({ page }) => {
        const chat = new ChatPage(page)
        await loadChatScript(page, 'happy-path')
        // Park after the chunk step (before usage) — the reply stays in the
        // streaming bucket with the cursor up, frozen indefinitely.
        await page.evaluate(() => {
          (window as unknown as {
            __shannonMock?: { control: { pauseAt(i: number): void } }
          }).__shannonMock?.control.pauseAt(1)
        })
        await chat.send('帮我写一首关于海的短诗')
        await expectMockPhase(page, 'waitingUi')
        await chat.expectStreamingCursor()
        await expect(chat.stopButton()).toBeVisible()
        await expect(page.locator('html')).toHaveAttribute('data-theme-mode', theme.mode)
        await page.addStyleTag({ content: STOP_ANIMATIONS })
        await page.waitForTimeout(400)
        await expect(page).toHaveScreenshot(`chat-matrix-streaming-${theme.mode}.png`, {
          maxDiffPixelRatio: 0.05,
          mask: DYNAMIC.map(f => page.locator(f())),
        })
      })

      test(`tool-running state [${theme.mode}]`, async ({ page }) => {
        const chat = new ChatPage(page)
        await loadChatScript(page, 'visual-tool-run')
        await chat.send('帮我看看目录结构')
        await expectMockPhase(page, 'waitingUi', 15_000)
        const runningCard = page.locator('[data-tool-name="Bash"][data-tool-status="running"]')
        await expect(runningCard).toBeVisible({ timeout: 10_000 })
        await expect(page.getByTestId('run-progress-pct')).toContainText('40%')
        await expect(page.locator('html')).toHaveAttribute('data-theme-mode', theme.mode)
        await page.addStyleTag({ content: STOP_ANIMATIONS })
        await page.waitForTimeout(400)
        await expect(page).toHaveScreenshot(`chat-matrix-tool-running-${theme.mode}.png`, {
          maxDiffPixelRatio: 0.05,
          mask: DYNAMIC.map(f => page.locator(f())),
        })
      })

      test(`approval-dialog state [${theme.mode}]`, async ({ page }) => {
        await loadChatScript(page, 'approval-allow')
        // Open the seeded session so the run carries a real session_id
        // (same boot semantics as the approval journey spec).
        await page.getByTestId('desktop-session-row-script-sess-approval').click()
        const chat = new ChatPage(page)
        await chat.send('运行 ls -la 看看当前目录里有什么')
        await expectMockPhase(page, 'waitingPermission', 15_000)
        const dialog = page.getByRole('alertdialog')
        await expect(dialog).toBeVisible({ timeout: 10_000 })
        await expect(dialog.getByText('Permission Request')).toBeVisible()
        await expect(page.locator('html')).toHaveAttribute('data-theme-mode', theme.mode)
        await page.addStyleTag({ content: STOP_ANIMATIONS })
        await page.waitForTimeout(400)
        await expect(page).toHaveScreenshot(`chat-matrix-approval-${theme.mode}.png`, {
          maxDiffPixelRatio: 0.05,
          mask: DYNAMIC.map(f => page.locator(f())),
        })
      })

      test(`error-banner state [${theme.mode}]`, async ({ page }) => {
        const chat = new ChatPage(page)
        await loadChatScript(page, 'mid-stream-fail')
        await chat.send('给我讲一个关于海的故事')
        await expect(page.getByText('upstream connection reset while streaming')).toBeVisible({ timeout: 15_000 })
        await expect(page.getByRole('button', { name: 'Retry' })).toBeVisible()
        await expect(chat.streamingCursor()).toHaveCount(0)
        await expect(page.locator('html')).toHaveAttribute('data-theme-mode', theme.mode)
        await page.addStyleTag({ content: STOP_ANIMATIONS })
        await page.waitForTimeout(400)
        await expect(page).toHaveScreenshot(`chat-matrix-error-banner-${theme.mode}.png`, {
          maxDiffPixelRatio: 0.05,
          mask: DYNAMIC.map(f => page.locator(f())),
        })
      })

      test(`settled state [${theme.mode}]`, async ({ page }) => {
        const chat = new ChatPage(page)
        await loadChatScript(page, 'happy-path')
        await chat.send('帮我写一首关于海的短诗')
        await expect(chat.sendButton()).toBeVisible({ timeout: 15_000 })
        await expect(chat.streamingCursor()).toHaveCount(0)
        await expect(chat.bubbles()).toHaveCount(2)
        await expect(chat.runStatusLine()).toHaveCount(0)
        await expect(page.locator('html')).toHaveAttribute('data-theme-mode', theme.mode)
        await page.addStyleTag({ content: STOP_ANIMATIONS })
        await page.waitForTimeout(400)
        await expect(page).toHaveScreenshot(`chat-matrix-settled-${theme.mode}.png`, {
          maxDiffPixelRatio: 0.05,
          mask: DYNAMIC.map(f => page.locator(f())),
        })
      })
    })
  }
})
