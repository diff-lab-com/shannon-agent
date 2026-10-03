// W2 journey #20（§9.4，gap G8+G9）— dock-interactions: the right dock's
// resize/fullscreen/persistence chrome, the one-time Ctrl+\ hint, two of the
// four auto-dock sources (plan-mode entry, Diff click), the PlanPanel human
// check write-back through save_text_file (+ the scripted-failure rollback),
// and the chat-fence HTML artifact's honest static fallback.
import { expect, test, type Page } from '@playwright/test'

import { ChatPage } from './helpers/ChatPage'
import { expectMockPhase, loadChatScript, loadChatScriptObject, readChatScript } from './helpers/scriptLoader'
import { expectNoConsoleErrors } from './helpers/watchdog'
import type { ChatScript } from '../src/lib/mock/scripted/schema'

const script = readChatScript('dock-interactions') as ChatScript
const WORKING_DIR = '/Users/demo/workspace/shannon-demo'

function dock(page: Page) {
  return page.getByRole('complementary', { name: 'Right dock' })
}
function dockWidth(page: Page): Promise<string> {
  return dock(page).evaluate((el) => (el as HTMLElement).style.width)
}
/** Read a localStorage key (persistence assertions). */
function stored(page: Page, key: string): Promise<string | null> {
  return page.evaluate((k) => localStorage.getItem(k), key)
}

/**
 * Bounded re-press anchor for the plan-mode shortcut.
 *
 * WHY: a one-shot Ctrl+Shift+P press is lossy under CI-grade CPU
 * starvation — the composer anchor below proves ChatInput is mounted, but
 * the press itself can still be dropped before it reaches ChatInput's
 * window keydown handler (observed in CI: no `configure` invoke,
 * `approval_mode` never armed, banner never renders — #239, second
 * occurrence). A real user simply presses the shortcut again; this anchor
 * does exactly that, BOUNDED: at most 3 presses total, and every re-press
 * first requires the banner to still be absent — a WORKING toggle is never
 * double-fired, because the second press of an already-armed toggle would
 * exit plan mode. The boundedness is the honesty guarantee: a permanently
 * dead shortcut exhausts both the press budget and the finite `toPass`
 * window, so the test still fails — the anchor cannot mask a real
 * regression.
 */
async function pressPlanModeShortcut(page: Page): Promise<void> {
  const banner = page.getByTestId('plan-mode-banner')
  let presses = 0
  await expect(async () => {
    if (presses < 3 && !(await banner.isVisible())) {
      await page.keyboard.press('Control+Shift+p')
      presses += 1
    }
    await expect(banner).toBeVisible({ timeout: 6_000 })
  }).toPass({ timeout: 30_000 })
}
async function openSession(page: Page): Promise<void> {
  await page.getByTestId('desktop-session-row-script-sess-dock').click()
  await expect(page.getByRole('heading', { name: 'Dock journey' })).toBeVisible({ timeout: 10_000 })
  // The Header heading alone does NOT prove the chat page is interactive:
  // it renders from `sessions.find(currentSessionId)` the moment the boot
  // binding lands, while the lazy Chat chunk (and ChatInput with it) may
  // still be compiling under load. Both one-shot Ctrl+Shift+P presses below
  // need ChatInput's window keydown handler — a press dispatched before it
  // mounts is silently dropped (observed in CI: no `configure` invoke, plan
  // mode never armed, dock never auto-opened). Anchor the composer itself.
  await expect(page.getByRole('textbox', { name: 'Message' })).toBeVisible({ timeout: 15_000 })
}

test.describe('scripted chat backend — dock-interactions (journey #20)', () => {
  test('keyboard resize clamps 280-720, fullscreen exits with the dock, shannon.dock.* survives a reload, the hint dismisses once', async ({ page }) => {
    test.setTimeout(90_000)
    await loadChatScript(page, 'dock-interactions', test.info())
    await openSession(page)

    // Fresh context: the dock boots closed (persisted open flag), the header
    // toggle opens it at the default width and the one-time hint shows.
    await page.getByRole('button', { name: 'Toggle context panel' }).click()
    const panel = dock(page)
    await expect(panel).toBeVisible({ timeout: 5_000 })
    await expect(panel).toHaveCSS('width', '340px')
    await expect(page.getByTestId('dock-shortcut-hint')).toBeVisible()

    // Picking ANY tab dismisses the hint (engagement is the signal).
    await page.getByRole('tab', { name: 'Plan' }).click()
    await expect(page.getByTestId('dock-shortcut-hint')).toHaveCount(0)

    // Keyboard resize: the separator widens on ArrowLeft (16px/step)…
    const separator = page.getByRole('separator', { name: 'Resize dock' })
    await separator.focus()
    for (let i = 0; i < 3; i++) await page.keyboard.press('ArrowLeft')
    await expect(panel).toHaveCSS('width', '388px', { timeout: 5_000 })

    // …and clamps both ways: 60% viewport (768) caps at 720, floor 280.
    for (let i = 0; i < 40; i++) await page.keyboard.press('ArrowLeft')
    await expect(panel).toHaveCSS('width', '720px', { timeout: 5_000 })
    for (let i = 0; i < 60; i++) await page.keyboard.press('ArrowRight')
    await expect(panel).toHaveCSS('width', '280px', { timeout: 5_000 })
    expect(await stored(page, 'shannon.dock.width')).toBe('280')

    // Reload: width, open flag AND the dismissed hint stay dismissed.
    await page.reload()
    await expect(panel).toBeVisible({ timeout: 10_000 })
    await expect(panel).toHaveCSS('width', '280px', { timeout: 10_000 })
    await expect(page.getByTestId('dock-shortcut-hint')).toHaveCount(0)

    // Fullscreen in… (the button's aria-label swaps to "Exit fullscreen"
    // while active, so the locator matches both labels.)
    const fullscreenButton = page.getByRole('button', { name: /fullscreen/i })
    await fullscreenButton.click()
    await expect(fullscreenButton).toHaveAttribute('aria-pressed', 'true')
    await expect(dock(page)).toHaveClass(/fixed/, { timeout: 5_000 })
    expect(await stored(page, 'shannon.dock.fullscreen')).toBe('1')

    // …persists over a reload…
    await page.reload()
    await expect(dock(page)).toHaveClass(/fixed/, { timeout: 10_000 })

    // …and closing the dock exits fullscreen (no empty overlay ever
    // outlives its content); reopening shows the normal docked form.
    await page.getByRole('button', { name: 'Close dock' }).click()
    await expect(dock(page)).toHaveCSS('width', '0px', { timeout: 5_000 })
    expect(await stored(page, 'shannon.dock.fullscreen')).toBe('0')
    await page.getByRole('button', { name: 'Toggle context panel' }).click()
    await expect(dock(page)).not.toHaveClass(/fixed/, { timeout: 5_000 })
    await expectNoConsoleErrors(page)
  })

  test('plan-mode entry auto-docks the plan tab; the human check writes back; a Diff click docks the review; html falls back static', async ({ page }) => {
    test.setTimeout(90_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'dock-interactions', test.info())
    await openSession(page)

    // Auto-dock source 1: entering plan mode (Ctrl+Shift+P) opens the dock
    // ON the plan tab and shows the composer banner.
    await pressPlanModeShortcut(page)
    await expect(page.getByText('Plan mode active — the agent will propose a plan before any file changes.')).toBeVisible({ timeout: 10_000 })
    await expect(dock(page)).toBeVisible({ timeout: 5_000 })
    await expect(page.getByRole('tab', { name: 'Plan' })).toHaveAttribute('aria-selected', 'true')
    await expect(page.getByTestId('plan-panel')).toBeVisible()
    // The demo plan checklist: 1/3 steps done, three checkboxes.
    await expect(page.getByRole("status", { name: /Plan progress: 1 of 3 steps done/ })).toBeVisible()
    const boxes = page.locator('[data-testid="plan-panel"] input[type="checkbox"]')
    await expect(boxes).toHaveCount(3)

    // Human verification: tick the second step — optimistic tick, armed
    // save_text_file records it, and the plan READ serves the written
    // content back, so the tick survives its own refresh.
    await boxes.nth(1).click()
    await expect(boxes.nth(1)).toBeChecked()
    await expect(page.getByRole("status", { name: /Plan progress: 2 of 3 steps done/ })).toBeVisible({ timeout: 10_000 })
    await expect
      .poll(async () => page.evaluate(async (dir) => {
        return (window as unknown as {
          __TAURI_INTERNALS__: { invoke(cmd: string, args?: Record<string, unknown>): Promise<{ content: string }> }
        }).__TAURI_INTERNALS__.invoke('get_session_plan', { workingDir: dir })
      }, WORKING_DIR), { timeout: 10_000 })
      .toMatchObject({ content: expect.stringContaining('- [x] Draft the OAuth gallery spec') })

    // Auto-dock source 2: the seeded write_file FileCard's "Review changes"
    // docks the single-file diff review on the Diff tab.
    await page.getByTestId('file-card-review-diff').click()
    await expect(page.getByRole('tab', { name: 'Diff' })).toHaveAttribute('aria-selected', 'true', { timeout: 10_000 })

    // The chat-fence html artifact chip docks its own tab; the interactive
    // registration fails in demo (no scripting host) → the honest static
    // fallback hint renders.
    await page.getByTestId('artifact-card').click()
    await expect(page.getByTestId('artifact-html-static-hint')).toBeVisible({ timeout: 10_000 })
    await expect(page.getByTestId('artifact-html-static-hint')).toContainText('could not run interactively')

    // A plan-tool result refreshes the plan panel and the turn settles.
    await page.getByRole('tab', { name: 'Plan' }).click()
    await chat.send(script.turns[0]!.user)
    await expectMockPhase(page, 'waitingUi', 15_000)
    await page.evaluate(() => {
      (window as unknown as {
        __shannonMock: { control: { resume(): void } }
      }).__shannonMock.control.resume()
    })
    await expect(chat.sendButton()).toBeVisible({ timeout: 15_000 })
    // A new run auto-activates the 运行 tab (GB P2-3) — back to the plan
    // tab before reading the checklist: the written tick survived.
    await page.getByRole('tab', { name: 'Plan' }).click()
    await expect(boxes.nth(1)).toBeChecked()
    await expectNoConsoleErrors(page)
  })

  test('the scripted write failure rolls the checkbox back to engine truth', async ({ page }) => {
    test.setTimeout(90_000)
    const chat = new ChatPage(page)
    // Failure injection arm: the same journey YAML with
    // config.saveTextFileFails — save_text_file rejects, the optimistic tick
    // rolls back, and the plan read keeps the ENGINE's content.
    const failing: ChatScript = {
      ...script,
      seed: { ...script.seed, config: { ...script.seed?.config, saveTextFileFails: true } },
    }
    await loadChatScriptObject(page, failing, test.info())
    await openSession(page)

    await pressPlanModeShortcut(page)
    await expect(page.getByTestId('plan-panel')).toBeVisible({ timeout: 10_000 })
    const boxes = page.locator('[data-testid="plan-panel"] input[type="checkbox"]')
    await expect(boxes).toHaveCount(3)

    await boxes.nth(1).click()
    // The optimistic tick flashes, then the failed write rolls it back —
    // the progress bar never leaves 1/3 and the file keeps the engine copy.
    await expect(boxes.nth(1)).not.toBeChecked({ timeout: 10_000 })
    await expect(page.getByRole("status", { name: /Plan progress: 1 of 3 steps done/ })).toBeVisible()
    await expect
      .poll(async () => page.evaluate(async (dir) => {
        return (window as unknown as {
          __TAURI_INTERNALS__: { invoke(cmd: string, args?: Record<string, unknown>): Promise<{ content: string }> }
        }).__TAURI_INTERNALS__.invoke('get_session_plan', { workingDir: dir })
      }, WORKING_DIR), { timeout: 10_000 })
      .toMatchObject({ content: expect.stringContaining('- [ ] Draft the OAuth gallery spec') })

    // The journey's own turn still runs to completion under the failure.
    await chat.send(script.turns[0]!.user)
    await expectMockPhase(page, 'waitingUi', 15_000)
    await page.evaluate(() => {
      (window as unknown as {
        __shannonMock: { control: { resume(): void } }
      }).__shannonMock.control.resume()
    })
    await expect(chat.sendButton()).toBeVisible({ timeout: 15_000 })
    await expectNoConsoleErrors(page)
  })
})
