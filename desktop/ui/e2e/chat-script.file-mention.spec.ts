// wave-2 journey #16 — file-mention (composer/input domain).
//
// The @ file-reference popover over the composer:
//   - candidates come from get_file_tree (the demo tree: src/main.rs,
//     src/lib.rs, README.md) plus the session file index (list_file_index);
//   - fuzzy ranking (basename prefix > substring > subsequence), keyboard
//     navigation (ArrowDown/Up + aria-selected), Enter/Tab commits;
//   - the insertion is PLAIN TEXT `@relative/path ` with the caret after
//     the trailing space — no attachment pipeline, no preflight;
//   - Escape dismisses for the current query and re-arms on a new one;
//     mid-text mentions work (caret-relative detection); email-shaped
//     @tokens never trigger;
//   - the e2e anchor: the sent message carries the @path verbatim (the
//     model reads the path in the message — turn 1 of the script).
import { expect, test } from '@playwright/test'

import { ChatPage } from './helpers/ChatPage'
import { loadChatScript } from './helpers/scriptLoader'
import { expectNoConsoleErrors } from './helpers/watchdog'
import { mockSnapshot } from './helpers/knownIssues'

const menu = (page: import('@playwright/test').Page) => page.getByRole('listbox', { name: 'File mentions' })

async function composerCaret(page: import('@playwright/test').Page): Promise<number> {
  return page.evaluate(() => {
    const el = document.querySelector('textarea[aria-label="Message"]') as HTMLTextAreaElement | null
    return el ? el.selectionStart : -1
  })
}

test.describe('scripted chat backend — file-mention (journey #16)', () => {
  test('@ opens the menu over tree+index candidates, ranks fuzzily, and keyboard-picks the path', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'file-mention', test.info())
    await expect(chat.composer()).toBeVisible({ timeout: 10_000 })

    // Bare '@': the full candidate universe — the working-dir tree first,
    // then the demo file index (newest first).
    await chat.composer().pressSequentially('check @')
    await expect(menu(page)).toBeVisible({ timeout: 5_000 })
    await expect(menu(page).getByRole('option')).toHaveCount(6)
    await expect(menu(page).getByRole('option').first()).toContainText('src/main.rs')

    // Fuzzy ranking: a basename-prefix query narrows to src/main.rs.
    await chat.composer().pressSequentially('ma')
    await expect(menu(page).getByRole('option')).toHaveCount(1)
    await expect(menu(page).getByRole('option')).toContainText('src/main.rs')

    // Keyboard navigation with several matches: '@re' ranks README.md first
    // (basename prefix), ArrowDown moves the aria-selected cursor to the
    // basename-substring row below it.
    await chat.composer().fill('check @re')
    await expect(menu(page).getByRole('option').first()).toContainText('README.md')
    await chat.composer().press('ArrowDown')
    const selected = menu(page).getByRole('option', { selected: true })
    await expect(selected).toContainText('q3-review.pptx')
    await chat.composer().press('ArrowUp')
    await expect(menu(page).getByRole('option', { selected: true })).toContainText('README.md')

    // Enter commits the active row: PLAIN TEXT `@README.md ` with the caret
    // parked after the trailing space — no chip, no attachment state.
    await chat.composer().press('Enter')
    await expect(menu(page)).toHaveCount(0)
    await expect(chat.composer()).toHaveValue('check @README.md ')
    expect(await composerCaret(page)).toBe('check @README.md '.length)
    await expectNoConsoleErrors(page)
  })

  test('a mid-text mention inserts without clobbering the tail; email @tokens never trigger', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'file-mention', test.info())
    await expect(chat.composer()).toBeVisible({ timeout: 10_000 })

    // Email-shaped @ (mid-word, no preceding whitespace) never opens the menu.
    await chat.composer().fill('user@example.com')
    await expect(menu(page)).toHaveCount(0)

    // Mid-text: type the tail first, then the @query in the middle — the
    // menu detector reads the text BEFORE the caret, so the query opens with
    // ' world' still ahead of it.
    await chat.composer().fill('')
    await chat.composer().pressSequentially('hello ')
    await chat.composer().pressSequentially('@read')
    // (the tail ' world' is inserted after the query below)
    await expect(menu(page).getByRole('option').first()).toContainText('README.md')
    await chat.composer().pressSequentially(' world')
    // The trailing whitespace dissolved the query; move the caret back into
    // the token and type one more character — a real keystroke recomputes
    // the caret-relative detector mid-text.
    for (let i = 0; i < 6; i++) await chat.composer().press('ArrowLeft')
    await expect(menu(page)).toHaveCount(0)
    await chat.composer().press('Backspace')
    await expect(menu(page).getByRole('option').first()).toContainText('README.md')
    await chat.composer().press('Enter')
    await expect(chat.composer()).toHaveValue('hello @README.md world')
    await expectNoConsoleErrors(page)
  })

  test('Escape dismisses for the current query and a new @query re-arms the menu', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'file-mention', test.info())
    await expect(chat.composer()).toBeVisible({ timeout: 10_000 })

    await chat.composer().fill('@ma')
    await expect(menu(page)).toBeVisible()
    await chat.composer().press('Escape')
    await expect(menu(page)).toHaveCount(0)
    // The same query stays dismissed even when re-typed via editing.
    await chat.composer().press('End')
    await expect(menu(page)).toHaveCount(0)

    // The dismissal HOLDS while the query is merely edited into a different
    // query — the protocol re-arms only once the query dissolves.
    await chat.composer().fill('try @li')
    await expect(menu(page)).toHaveCount(0)
    // Whitespace after the token dissolves the query; the next @query opens
    // fresh and Tab commits it.
    await chat.composer().pressSequentially(' @li')
    await expect(menu(page)).toBeVisible({ timeout: 5_000 })
    await expect(menu(page).getByRole('option')).toContainText('src/lib.rs')
    await chat.composer().press('Tab')
    await expect(chat.composer()).toHaveValue('try @li @src/lib.rs ')
    await expectNoConsoleErrors(page)
  })

  test('the sent message carries the @path verbatim (plain text, no attachment pipeline)', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'file-mention', test.info())
    await expect(chat.composer()).toBeVisible({ timeout: 10_000 })

    await chat.composer().pressSequentially('check @ma')
    await expect(menu(page).getByRole('option')).toContainText('src/main.rs')
    await chat.composer().press('Enter')
    await expect(chat.composer()).toHaveValue('check @src/main.rs ')

    // The e2e anchor: the mention rides in the message TEXT. No rejected-
    // attachments receipt, no attachment paths on the wire — different
    // pipeline from the attachments journey (which asserted both).
    await chat.composer().press('Enter')
    await expect(chat.bubbles()).toHaveCount(2, { timeout: 15_000 })
    await expect(chat.bubbleAt(0)).toContainText('@src/main.rs')
    const snapshot = await mockSnapshot(page)
    // The wire message is the trimmed input (the trailing insert-space is
    // composer padding, not content); attachments stay null — different
    // pipeline from the attachments journey.
    expect(snapshot.sends[0]).toMatchObject({ message: 'check @src/main.rs', attachments: null })
    await expect(chat.bubbleAt(1)).toContainText('src/main.rs 的引用')
    await expectNoConsoleErrors(page)
  })
})
