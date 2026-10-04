// W2 journey #19（§9.4，gap G7）— session-lifecycle: the sidebar session
// rail's full lifecycle over a seeded 62-session roster (55 cap filler + 7
// interactive). Pinned: inline rename, pin (+ localStorage persistence over
// a reload), grouping-mode persistence, archive → archived section →
// restore, DeleteSessionModal (success closes / deleteFails fixture keeps it
// open / the archived lens' permanent variant), the debounced title search
// (backend seed branch ≥3 chars, client filter below), and the 50-row
// visible cap with its Show-all expander.
import { expect, test, type Page } from '@playwright/test'

import { ChatPage } from './helpers/ChatPage'
import { loadChatScript } from './helpers/scriptLoader'
import { expectNoConsoleErrors } from './helpers/watchdog'

const MAIN = 'script-sess-life-main'

function row(page: Page, id: string) {
  return page.getByTestId(`desktop-session-row-${id}`)
}
/** The row's ⋯ actions menu (rendered on hover/focus — click force-hovers). */
async function openRowMenu(page: Page, id: string, title: string): Promise<void> {
  await row(page, id).hover()
  await page.getByRole('button', { name: `Actions for ${title}` }).click()
}

test.describe('scripted chat backend — session-lifecycle (journey #19)', () => {
  test('rename, pin and grouping preference survive a reload (localStorage keys)', async ({ page }) => {
    test.setTimeout(90_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'session-lifecycle', test.info())
    await row(page, MAIN).click()
    await expect(page.getByRole('heading', { name: 'Lifecycle main' })).toBeVisible({ timeout: 10_000 })
    await chat.send('生命周期旅程：确认当前会话可用')
    await expect(chat.sendButton()).toBeVisible({ timeout: 15_000 })

    // Inline rename through the row's ⋯ menu. The rename lands backend-side
    // (the armed seed registry) and the rail re-reads it.
    await openRowMenu(page, 'script-sess-life-rename', 'Rename me')
    await page.getByRole('menuitem', { name: 'Rename' }).click()
    const renameInput = page.getByRole('textbox', { name: 'Rename' })
    await expect(renameInput).toBeVisible()
    await renameInput.fill('Renamed by journey')
    await renameInput.press('Enter')
    await expect(row(page, 'script-sess-life-rename')).toContainText('Renamed by journey', { timeout: 10_000 })

    // Pin: the rail writes shannon-sessions-pinned…
    await openRowMenu(page, 'script-sess-life-pin', 'Pin me')
    await page.getByRole('menuitem', { name: 'Pin', exact: true }).click()
    const pinned = await page.evaluate(() => localStorage.getItem('shannon-sessions-pinned'))
    expect(pinned).toContain('script-sess-life-pin')

    // Grouping switch → shannon-sessions-grouping (raw string, not JSON).
    await page.getByRole('button', { name: 'Flat by session' }).click()
    expect(await page.evaluate(() => localStorage.getItem('shannon-sessions-grouping'))).toBe('session')

    // Reload — the script re-arms from the injected boot global; the
    // localStorage-backed preferences must survive (the seeded rename is
    // runtime state and resets with the script: recorded in the report).
    await page.reload()
    await expect(row(page, MAIN)).toBeVisible({ timeout: 10_000 })
    // The pin marker (material ligature) renders on the persisted pin.
    await expect(row(page, 'script-sess-life-pin')).toContainText('push_pin')
    // The grouping toggle is still the persisted mode.
    await expect(page.getByRole('button', { name: 'Flat by session' })).toHaveAttribute('aria-pressed', 'true')
    // The renamed title reverted with the fresh script lifecycle (the rename
    // is seed-runtime, not localStorage) — current behavior, recorded.
    await expect(row(page, 'script-sess-life-rename')).toContainText('Rename me')
    await expectNoConsoleErrors(page)
  })

  test('archive → archived section → restore; delete closes; refused delete keeps the dialog; permanent variant', async ({ page }) => {
    test.setTimeout(90_000)
    await loadChatScript(page, 'session-lifecycle', test.info())
    await row(page, MAIN).click()
    await expect(page.getByRole('heading', { name: 'Lifecycle main' })).toBeVisible({ timeout: 10_000 })

    // Archive: the row leaves the rail, the archived section appears
    // (collapsed by default on a non-empty rail) and lists the row.
    await openRowMenu(page, 'script-sess-life-archive', 'Archive me')
    await page.getByRole('menuitem', { name: 'Archive' }).click()
    await expect(page.getByText('Session archived')).toBeVisible({ timeout: 5_000 })
    await expect(row(page, 'script-sess-life-archive')).toHaveCount(0, { timeout: 10_000 })
    await expect(page.getByTestId('sidebar-archived-section')).toBeVisible()
    await page.getByTestId('sidebar-archived-toggle').click()
    await expect(page.getByTestId('archived-row-script-sess-life-archive')).toBeVisible()

    // Restore: the row is rebuilt on the rail (toast is the audible half).
    await page.getByTestId('archived-restore-script-sess-life-archive').click()
    await expect(page.getByText('Session restored')).toBeVisible({ timeout: 5_000 })
    await expect(row(page, 'script-sess-life-archive')).toBeVisible({ timeout: 10_000 })

    // Plain delete: the dialog names the target; confirming closes it and
    // the row leaves for good.
    await openRowMenu(page, 'script-sess-life-delete', 'Delete me')
    await page.getByRole('menuitem', { name: 'Delete' }).click()
    const dialog = page.getByRole('alertdialog')
    await expect(dialog).toBeVisible()
    await expect(dialog.getByText('Delete Chat')).toBeVisible()
    await expect(dialog.getByText('Delete “Delete me”')).toBeVisible()
    await page.getByTestId('delete-session-confirm').click()
    await expect(dialog).toHaveCount(0, { timeout: 10_000 })
    await expect(row(page, 'script-sess-life-delete')).toHaveCount(0, { timeout: 10_000 })

    // Refused delete (the deleteFails fixture): the dialog STAYS open — the
    // failure surfaces through the shared error banner — and Cancel backs
    // out without a second confirm.
    await openRowMenu(page, 'script-sess-life-cursed', 'Cursed delete')
    await page.getByRole('menuitem', { name: 'Delete' }).click()
    await expect(dialog).toBeVisible()
    await page.getByTestId('delete-session-confirm').click()
    await expect(page.getByRole('alert').filter({ hasText: 'delete refused' })).toBeVisible({ timeout: 10_000 })
    await expect(page.getByTestId('delete-session-confirm')).toBeVisible()
    await expect(page.getByTestId('delete-session-confirm')).toBeEnabled()
    await dialog.getByRole('button', { name: 'Cancel' }).click()
    await expect(dialog).toHaveCount(0)
    await expect(row(page, 'script-sess-life-cursed')).toBeVisible()

    // Permanent variant from the archived lens: archive the rename row, then
    // delete it from the 已归档 section — starker copy, row leaves the lens.
    await openRowMenu(page, 'script-sess-life-rename', 'Rename me')
    await page.getByRole('menuitem', { name: 'Archive' }).click()
    await expect(page.getByTestId('archived-row-script-sess-life-rename')).toBeVisible({ timeout: 10_000 })
    await page.getByTestId('archived-delete-script-sess-life-rename').click()
    await expect(dialog.getByText('Permanently delete chat')).toBeVisible()
    await page.getByTestId('delete-session-confirm').click()
    await expect(page.getByTestId('archived-row-script-sess-life-rename')).toHaveCount(0, { timeout: 10_000 })
    await expect(dialog).toHaveCount(0)
    await expectNoConsoleErrors(page)
  })

  // B3-5 pin (plan §七-3 / B1-3 journey half): a typed-but-unsent draft must
  // not outlive its session — deleteSessionAction clears the session's
  // `shannon.draft.<id>` key, and the AppContext-level unit test
  // (AppContextB1.test.tsx) pins the storage half; these journeys pin the
  // real-page interaction, where Chat's debounced/switch-flush draft writers
  // are also live while a session goes away underneath them.
  //
  // B1-3-RESIDUE (found by B3-5, since fixed): deleting the CURRENTLY OPEN
  // session used to re-write the key after deleteSessionAction's clearDraft —
  // the deleted id rode out through Chat's switch-flush (visibleSessionId →
  // null with the text still in the composer ⇒ persistDraft(previousId,
  // input)), resurrecting `shannon.draft.<id>` as stale residue. The fix
  // tombstones the id on delete at the write layer (lib/composerDraft), so
  // the flush and the straddling debounce are both refused; the second
  // journey below asserts the key is GONE for that OPEN-session shape.
  test('deleting a session from another session clears its typed draft (B1-3: no shannon.draft.<id> residue)', async ({ page }) => {
    test.setTimeout(90_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'session-lifecycle', test.info())
    await row(page, MAIN).click()
    await expect(page.getByRole('heading', { name: 'Lifecycle main' })).toBeVisible({ timeout: 10_000 })

    // A draft typed but never sent — the debounced (300ms) write lands it
    // under the session's key (polled: the debounce is not a fixed sleep).
    const DRAFT_KEY = `shannon.draft.${MAIN}`
    await chat.composer().fill('删除我之前没发出去的草稿')
    await expect
      .poll(async () => page.evaluate(k => localStorage.getItem(k), DRAFT_KEY), { timeout: 5_000 })
      .toContain('删除我之前没发出去的草稿')

    // Move to ANOTHER session first (the switch flush keeps MAIN's draft on
    // disk — that is the persistence contract, not residue), then delete
    // MAIN from the rail: ⋯ menu → Delete → confirm.
    await row(page, 'script-sess-life-rename').click()
    await expect(page.getByRole('heading', { name: 'Rename me' })).toBeVisible({ timeout: 10_000 })
    await openRowMenu(page, MAIN, 'Lifecycle main')
    await page.getByRole('menuitem', { name: 'Delete' }).click()
    const dialog = page.getByRole('alertdialog')
    await expect(dialog).toBeVisible()
    await page.getByTestId('delete-session-confirm').click()
    await expect(dialog).toHaveCount(0, { timeout: 10_000 })
    await expect(row(page, MAIN)).toHaveCount(0, { timeout: 10_000 })

    // The draft key died with the session — clearDraft is the last writer
    // (no switch-flush targets a deleted id from here).
    expect(await page.evaluate(k => localStorage.getItem(k), DRAFT_KEY)).toBeNull()
    await expectNoConsoleErrors(page)
  })

  // The same journey with the deletion fired from the deleted session's OWN
  // view — the exact shape that used to resurrect the key (B1-3-RESIDUE,
  // fixed: the delete tombstones the id, so the switch-flush write is
  // dropped at the write layer).
  test('deleting the OPEN session clears its typed draft too (B1-3-RESIDUE: no residue)', async ({ page }) => {
    test.setTimeout(90_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'session-lifecycle', test.info())
    await row(page, MAIN).click()
    await expect(page.getByRole('heading', { name: 'Lifecycle main' })).toBeVisible({ timeout: 10_000 })

    const DRAFT_KEY = `shannon.draft.${MAIN}`
    await chat.composer().fill('删除我之前没发出去的草稿')
    await expect
      .poll(async () => page.evaluate(k => localStorage.getItem(k), DRAFT_KEY), { timeout: 5_000 })
      .toContain('删除我之前没发出去的草稿')

    await openRowMenu(page, MAIN, 'Lifecycle main')
    await page.getByRole('menuitem', { name: 'Delete' }).click()
    const dialog = page.getByRole('alertdialog')
    await expect(dialog).toBeVisible()
    await page.getByTestId('delete-session-confirm').click()
    await expect(dialog).toHaveCount(0, { timeout: 10_000 })
    // The row leaving means the pointer flip already committed — the flush
    // writer has fired (and been refused) by the time this settles.
    await expect(row(page, MAIN)).toHaveCount(0, { timeout: 10_000 })

    // The key died with the session and stays dead: the switch-flush
    // straggler hits the tombstone, mirroring the clean path pinned above.
    expect(await page.evaluate(k => localStorage.getItem(k), DRAFT_KEY)).toBeNull()
    await expectNoConsoleErrors(page)
  })

  test('debounced title search filters the rail; the 50-row cap hides behind Show all', async ({ page }) => {
    test.setTimeout(90_000)
    await loadChatScript(page, 'session-lifecycle', test.info())
    // Bind the active session to a TOP-of-rail row first — the cold-start
    // active (the first seeded session) sorts LAST, and the visible cap
    // always keeps the active row, which would make the capped count 51.
    await row(page, MAIN).click()
    await expect(page.getByRole('heading', { name: 'Lifecycle main' })).toBeVisible({ timeout: 10_000 })

    // 62 seeded rows, 50 rendered — the cap expander is on.
    const rows = page.locator('[data-testid^="desktop-session-row-"]')
    await expect(rows).toHaveCount(50, { timeout: 10_000 })
    await expect(page.getByTestId('sidebar-show-all')).toBeVisible()
    await page.getByTestId('sidebar-show-all').click()
    await expect(rows).toHaveCount(62, { timeout: 10_000 })

    // ≥3 chars: the debounced (250ms) backend search answers from the seeded
    // roster — exactly one title match survives. (The rail's filter input is
    // type=search → role searchbox.)
    const search = page.getByRole('searchbox', { name: 'Search chats' })
    await search.fill('needle-xyzzy')
    await expect(rows).toHaveCount(1, { timeout: 10_000 })
    await expect(row(page, 'script-sess-life-needle')).toBeVisible()
    await expect(row(page, MAIN)).toHaveCount(0)

    // <3 chars: the instant client-side title filter (no backend round-trip).
    await search.fill('pi')
    await expect(rows).toHaveCount(1, { timeout: 10_000 })
    await expect(row(page, 'script-sess-life-pin')).toBeVisible()

    // Clearing restores the (still expanded) full roster.
    await search.fill('')
    await expect(rows).toHaveCount(62, { timeout: 10_000 })
    await expectNoConsoleErrors(page)
  })
})
