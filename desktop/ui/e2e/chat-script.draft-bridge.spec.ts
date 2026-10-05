// wave-2 journey #18 — draft-bridge (composer/input domain).
//
// The shannon:composer-draft CustomEvent is the seam between surface
// components and the composer. Its trust contract, end to end:
//   - NEVER auto-sends: a pushed draft only fills the composer (snapshot
//     sentTurns stays 0 until the user presses Enter themselves);
//   - appends, never overwrites: a second push joins the draft after a
//     blank line;
//   - real application surfaces ride the same path: the PPT outline dialog
//     (Generate pushes pptDraftOf) and the Context dock's Session sources
//     cite ([Source] line);
//   - the editor's dirty-close confirm gates the three close paths
//     (Esc, close button, backdrop → ConfirmDialog).
//
// The pending-draft queue half of the contract (push while unmounted →
// flush on mount) is pinned at L1 in chatStateMachine.journeys.composer —
// the raw window event has no subscriber queue by design, and every in-app
// pushComposerDraft surface lives on /chat (see report note).
import { expect, test } from '@playwright/test'

import { ChatPage } from './helpers/ChatPage'
import { loadChatScript } from './helpers/scriptLoader'
import { expectNoConsoleErrors } from './helpers/watchdog'
import { mockSnapshot } from './helpers/knownIssues'

async function pushDraft(page: import('@playwright/test').Page, text: string): Promise<void> {
  await page.evaluate((t) => {
    window.dispatchEvent(new CustomEvent('shannon:composer-draft', { detail: { text: t } }))
  }, text)
}

/**
 * Open the plus menu and pick an item by keyboard. The menu roves focus via
 * a post-commit effect (dropdown-menu.tsx: initial focus(0) flushes one
 * state update after the menu is visible), so an ArrowDown that lands before
 * that flush computes from -1 and the Enter then fires items[0] (attach)
 * instead of the intended item — exactly what a starved runner does. Wait
 * until the initial focus is OBSERVABLE on the first item before keying.
 */
async function openPlusMenuAndPick(page: import('@playwright/test').Page, arrowDowns: number): Promise<void> {
  await page.getByRole('button', { name: 'Attachments and tools' }).click()
  await expect(page.getByRole('menu', { name: 'Attachments and tools' })).toBeVisible({ timeout: 5_000 })
  await expect(page.locator('[data-menu-item-index="0"]')).toBeFocused({ timeout: 5_000 })
  for (let i = 0; i < arrowDowns; i++) await page.keyboard.press('ArrowDown')
  await page.keyboard.press('Enter')
}

test.describe('scripted chat backend — draft-bridge (journey #18)', () => {
  test('pushed drafts never auto-send and append instead of overwriting', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'draft-bridge', test.info())
    await expect(chat.composer()).toBeVisible({ timeout: 10_000 })

    // Pin 1 — never auto-send: the draft lands in the composer, nothing
    // leaves (the player stays armed, zero turns consumed, no bubbles).
    await pushDraft(page, 'First draft line')
    await expect(chat.composer()).toHaveValue('First draft line')
    let snapshot = await mockSnapshot(page)
    expect(snapshot.sentTurns).toBe(0)
    expect(snapshot.phase).toBe('armed')
    await expect(chat.bubbles()).toHaveCount(0)

    // Pin 2 — append, never overwrite: the second push joins the draft
    // (blank-line separated) instead of replacing it.
    await pushDraft(page, 'Second draft line')
    await expect(chat.composer()).toHaveValue('First draft line\n\nSecond draft line')
    snapshot = await mockSnapshot(page)
    expect(snapshot.sentTurns).toBe(0)

    // Only the user's explicit Enter sends — and it carries the whole draft.
    await chat.composer().press('Enter')
    await expect(chat.bubbles()).toHaveCount(2, { timeout: 15_000 })
    await expect(chat.bubbleAt(0)).toContainText('First draft line')
    await expect(chat.bubbleAt(0)).toContainText('Second draft line')
    expect((await mockSnapshot(page)).sentTurns).toBe(1)
    await expectNoConsoleErrors(page)
  })

  test('the PPT outline dialog pushes its draft through the same bridge and never sends', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'draft-bridge', test.info())
    await expect(chat.composer()).toBeVisible({ timeout: 10_000 })

    // Existing draft text proves the dialog APPENDS to it.
    await pushDraft(page, 'Meeting notes snippet')
    await expect(chat.composer()).toHaveValue('Meeting notes snippet')

    // The plus menu is keyboard-first: it focuses the first item on open,
    // so ArrowDown + Enter picks "Build a presentation" (pointer clicks are
    // flaky here — the item's box never settles under Playwright's
    // stability check while the composer re-renders). openPlusMenuAndPick
    // waits out the focus-roving effect before keying.
    await openPlusMenuAndPick(page, 1)
    const dialog = page.getByRole('dialog')
    await expect(dialog).toBeVisible({ timeout: 5_000 })
    await dialog.getByTestId('ppt-outline-input').fill('Title — Wave 2 journeys\nCoverage map')
    await dialog.getByRole('button', { name: 'Generate with agent' }).click()
    await expect(dialog).toHaveCount(0)

    // The composed outline draft joined the existing text; still nothing sent.
    await expect(chat.composer()).toHaveValue(/Meeting notes snippet\n\nBuild a presentation from exactly this outline \(one slide per line\):\nTitle — Wave 2 journeys\nCoverage map/)
    expect((await mockSnapshot(page)).sentTurns).toBe(0)
    await expectNoConsoleErrors(page)
  })

  test('Session sources cite pushes a [Source] draft line into the composer', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'draft-bridge', test.info())
    await expect(chat.composer()).toBeVisible({ timeout: 10_000 })

    // Open the right dock (Context tab is the default) and add a source.
    await page.getByRole('button', { name: 'Toggle context panel' }).click()
    const sources = page.getByTestId('session-sources')
    await expect(sources).toBeVisible({ timeout: 10_000 })
    await page.getByTestId('session-source-input').fill('https://example.com/wave2-spec')
    await page.getByTestId('session-source-add').click()
    await expect(page.getByTestId('session-source-item')).toHaveCount(1)

    // Cite → pushComposerDraft(`[Source] …`) → the composer holds the line.
    await page.getByTestId('session-source-cite').click()
    await expect(chat.composer()).toHaveValue('[Source] https://example.com/wave2-spec')
    // Still never-send: the draft waits for the user.
    expect((await mockSnapshot(page)).sentTurns).toBe(0)
    await expectNoConsoleErrors(page)
  })

  test('editor dirty-close confirm: Esc and the close button both ask before discarding', async ({ page }) => {
    test.setTimeout(90_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'draft-bridge', test.info())
    await expect(chat.composer()).toBeVisible({ timeout: 10_000 })

    // Open the chat-inline editor (plus menu, keyboard-first: ArrowDown ×3
    // → "Editor") and load a file (read_source_file mock).
    await openPlusMenuAndPick(page, 3)
    const modal = page.getByRole('dialog')
    await expect(modal).toBeVisible({ timeout: 10_000 })
    await page.evaluate(() => {
      window.dispatchEvent(new CustomEvent('shannon:open-code-file', {
        detail: { path: '/Users/demo/workspace/my-startup/README.md' },
      }))
    })
    await expect(modal.getByText('README.md', { exact: true })).toBeVisible({ timeout: 10_000 })

    // Make it dirty: toggle edit mode and type into the CodeMirror surface.
    await modal.getByRole('button', { name: 'Edit', exact: true }).click()
    const cmContent = modal.locator('.cm-content')
    await expect(cmContent).toBeVisible()
    await cmContent.click()
    await page.keyboard.press('Control+End')
    await page.keyboard.insertText('dirty edit')
    await expect(modal.getByRole('button', { name: 'Save' })).toBeVisible()

    // Close path 1 — Esc: the dirty guard intercepts with the confirm.
    await page.keyboard.press('Escape')
    await expect(page.getByRole('alertdialog')).toBeVisible({ timeout: 5_000 })
    // Keep editing: the dialog closes, the editor stays open with the draft.
    await page.getByRole('button', { name: 'Keep editing' }).click()
    await expect(page.getByRole('alertdialog')).toHaveCount(0)
    await expect(modal).toBeVisible()

    // Close path 2 — the backdrop: the guard asks again, and confirming
    // discards the draft and closes the editor.
    await page.mouse.click(15, 15)
    await expect(page.getByRole('alertdialog')).toBeVisible({ timeout: 5_000 })
    await page.getByRole('button', { name: 'Discard changes' }).click()
    await expect(modal).toHaveCount(0)
    await expectNoConsoleErrors(page)
  })
})
