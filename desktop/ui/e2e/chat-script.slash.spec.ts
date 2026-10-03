// wave-2 journey #15 — slash-commands (composer/input domain).
//
// The composer slash surface, end to end:
//   - /goal form: required-objective gating (Start disabled empty, inline
//     numeric-cap validation), then goal-start-success — with a session and,
//     in the no-session script, the backend-minted dedicated goal session.
//   - /diff: the autocomplete menu path plus the SlashResultCard's four
//     shapes — notRepo (the real handler default), noChanges / truncated /
//     full patch (the scripted `diff:<case>` seed sentinel fixtures).
//   - /export: the harness has no native save dialog (plugin-dialog is not
//     aliased in mock mode), so the honest surface is the failure toast.
//   - /new: creates a session and clears the conversation.
//   - /cost: the events:0 legacyHint line (context-panels pins the numbers).
//   - parse rules at the wire: `/name args` and unknown `/tokens` reach the
//     model as plain text (turn consumption + sends log).
import { expect, test } from '@playwright/test'

import { ChatPage } from './helpers/ChatPage'
import { loadChatScript } from './helpers/scriptLoader'
import { expectNoConsoleErrors } from './helpers/watchdog'
import { mockSnapshot } from './helpers/knownIssues'

test.describe('scripted chat backend — slash-commands (journey #15)', () => {
  test('/goal form: disabled start, inline cap validation, then goal-start-success', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'slash-commands', test.info())
    await expect(chat.composer()).toBeVisible({ timeout: 10_000 })
    await expect(page.getByRole('heading', { name: 'Slash commands' })).toBeVisible({ timeout: 10_000 })

    await chat.send('/goal')

    const card = page.getByRole('status').filter({ hasText: 'Start a goal run' })
    await expect(card).toBeVisible({ timeout: 10_000 })
    // Required-objective gate: the submit stays disabled while empty.
    const objective = card.getByPlaceholder(/Describe the completion condition/)
    await expect(objective).toBeVisible()
    const start = card.getByRole('button', { name: 'Start goal' })
    await expect(start).toBeDisabled()

    // Inline numeric-cap validation: a typo'd max-turns blocks the submit
    // with a role=alert line instead of silently starting an unlimited run.
    await objective.fill('ship the fix')
    const maxTurns = card.getByPlaceholder('unlimited')
    await maxTurns.fill('12x')
    await expect(start).toBeEnabled()
    await start.click()
    const alert = card.getByRole('alert')
    await expect(alert).toContainText('Enter a positive whole number')

    // A valid cap submits; the mock start_goal_run resolves for the seeded
    // session and the card flips to its success state.
    await maxTurns.fill('12')
    await start.click()
    await expect(card.getByTestId('goal-start-success')).toBeVisible({ timeout: 10_000 })
    await expect(card.getByText('Goal run started.')).toBeVisible()

    // Slash commands are local: nothing reached the scripted player.
    expect((await mockSnapshot(page)).sentTurns).toBe(0)
    await expectNoConsoleErrors(page)
  })

  test('/goal without any session: the backend mints a dedicated goal session', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    // seed.sessions: [] → the app boots unbound (no active session) and the
    // form submits with sessionId null.
    await loadChatScript(page, 'slash-goal-no-session', test.info())
    await expect(chat.composer()).toBeVisible({ timeout: 10_000 })

    await chat.send('/goal')
    const card = page.getByRole('status').filter({ hasText: 'Start a goal run' })
    await expect(card).toBeVisible({ timeout: 10_000 })
    const objective = card.getByPlaceholder(/Describe the completion condition/)
    await objective.fill('write the changelog from scratch')
    await card.getByRole('button', { name: 'Start goal' }).click()

    // start_goal_run(null) mints the dedicated goal session — the form's
    // success state proves the no-session branch resolves.
    await expect(card.getByTestId('goal-start-success')).toBeVisible({ timeout: 10_000 })
    expect((await mockSnapshot(page)).sentTurns).toBe(0)
    await expectNoConsoleErrors(page)
  })

  test('/diff via the autocomplete menu: keyboard-first selection runs the notRepo default card', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'slash-commands', test.info())
    await expect(chat.composer()).toBeVisible({ timeout: 10_000 })
    // The boot must bind the session BEFORE a slash command fires — the
    // slash result card is cleared whenever currentSessionId changes, so a
    // mid-boot command would be wiped by the settling session binding.
    await expect(page.getByRole('heading', { name: 'Slash commands' })).toBeVisible({ timeout: 10_000 })

    // Partial token opens the keyboard-first listbox; ArrowDown/ArrowUp move
    // the aria-selected cursor and Enter executes the active option (no
    // send_message). Pointer clicks on the menu are intercepted by the
    // welcome state's full-height empty overlay — an observed app quirk,
    // keyboard is the menu's primary interaction anyway.
    await chat.composer().fill('/di')
    const menu = page.getByRole('listbox', { name: 'Slash commands' })
    await expect(menu).toBeVisible({ timeout: 5_000 })
    await expect(menu.getByRole('option', { selected: true })).toContainText('/diff')
    await chat.composer().press('ArrowDown')
    // The cursor moves off /diff (filter matches diff / detect-skills /
    // editor for "/di") and ArrowUp returns to it.
    await expect(menu.getByRole('option', { selected: true })).not.toContainText('/diff')
    await chat.composer().press('ArrowUp')
    await expect(menu.getByRole('option', { selected: true })).toContainText('/diff')
    await chat.composer().press('Enter')

    const card = page.getByRole('status').filter({ hasText: 'Git diff' })
    await expect(card).toBeVisible({ timeout: 10_000 })
    // The demo backend has no repo: the honest calm notice (handler default).
    await expect(card.getByText('The working directory is not a git repository.')).toBeVisible()
    await expect(card.getByText('No uncommitted changes.')).toHaveCount(0)

    await page.getByRole('button', { name: 'Dismiss' }).click()
    await expect(card).toHaveCount(0)
    expect((await mockSnapshot(page)).sentTurns).toBe(0)
    await expectNoConsoleErrors(page)
  })

  test('/diff scripted fixtures: noChanges, truncated, and the patch expand/collapse', async ({ page }) => {
    test.setTimeout(90_000)

    // noChanges: a clean repo renders the quiet card, no file rows.
    await loadChatScript(page, 'slash-diff-nochanges', test.info())
    const chat = new ChatPage(page)
    await expect(chat.composer()).toBeVisible({ timeout: 10_000 })
    await expect(page.getByRole('heading', { name: 'Diff nochanges fixture' })).toBeVisible({ timeout: 10_000 })
    await chat.send('/diff')
    let card = page.getByRole('status').filter({ hasText: 'Git diff' })
    await expect(card.getByText('No uncommitted changes.')).toBeVisible({ timeout: 10_000 })
    await expect(card.getByRole('button', { name: 'Show patch' })).toHaveCount(0)

    // truncated: file rows plus the "patch truncated" flag.
    await loadChatScript(page, 'slash-diff-truncated', test.info())
    await expect(chat.composer()).toBeVisible({ timeout: 10_000 })
    await expect(page.getByRole('heading', { name: 'Diff truncated fixture' })).toBeVisible({ timeout: 10_000 })
    await chat.send('/diff')
    card = page.getByRole('status').filter({ hasText: 'Git diff' })
    await expect(card.getByText('3 files changed', { exact: false })).toBeVisible({ timeout: 10_000 })
    await expect(card.getByText('patch truncated')).toBeVisible()
    await expect(card.getByText('src/billing/invoice.rs')).toBeVisible()
    await card.getByRole('button', { name: 'Show patch' }).click()
    await expect(card.getByText(/patch capped by the backend/)).toBeVisible()

    // patch: rows with +x −y and the expandable CodeBlock.
    await loadChatScript(page, 'slash-diff-patch', test.info())
    await expect(chat.composer()).toBeVisible({ timeout: 10_000 })
    await expect(page.getByRole('heading', { name: 'Diff patch fixture' })).toBeVisible({ timeout: 10_000 })
    await chat.send('/diff')
    card = page.getByRole('status').filter({ hasText: 'Git diff' })
    await expect(card.getByText('2 files changed', { exact: false })).toBeVisible({ timeout: 10_000 })
    await expect(card.getByText('src/main.rs')).toBeVisible()
    await expect(card.getByText('+2')).toBeVisible()
    await expect(card.getByText('−1')).toBeVisible()
    await expect(card.getByText(/diff --git a\/src\/main\.rs/)).toHaveCount(0)
    await card.getByRole('button', { name: 'Show patch' }).click()
    await expect(card.getByText(/diff --git a\/src\/main\.rs/)).toBeVisible()
    await card.getByRole('button', { name: 'Hide patch' }).click()
    await expect(card.getByText(/diff --git a\/src\/main\.rs/)).toHaveCount(0)
    await expectNoConsoleErrors(page)
  })

  test('/export: the save dialog drives both halves — calm cancel, then success toast', async ({ page }) => {
    test.setTimeout(90_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'slash-commands', test.info())
    await expect(chat.composer()).toBeVisible({ timeout: 10_000 })
    await expect(page.getByRole('heading', { name: 'Slash commands' })).toBeVisible({ timeout: 10_000 })

    // Cancel half: the mocked dialog resolves null (session title not the
    // ExportSuccess sentinel) — the app treats it as a calm user cancel:
    // no toast, nothing sent.
    await chat.send('/export')
    await page.waitForTimeout(800)
    await expect(page.locator('[data-sonner-toast]')).toHaveCount(0)
    expect((await mockSnapshot(page)).sentTurns).toBe(0)

    // Success half: the ExportSuccess journey's seeded title makes the
    // dialog return a path; save_text_file lands and the success toast
    // names it.
    await loadChatScript(page, 'slash-export-success', test.info())
    await expect(chat.composer()).toBeVisible({ timeout: 10_000 })
    await expect(page.getByRole('heading', { name: 'ExportSuccess journey' })).toBeVisible({ timeout: 10_000 })
    await chat.send('/export')
    const toast = page.locator('[data-sonner-toast]').filter({ hasText: 'Exported to Markdown' })
    await expect(toast).toBeVisible({ timeout: 10_000 })
    await expect(toast).toContainText('/Users/demo/Downloads/ExportSuccess.md')
    expect((await mockSnapshot(page)).sentTurns).toBe(0)
    await expectNoConsoleErrors(page)
  })

  test('/new creates a session and clears the conversation without consuming a turn', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'slash-commands', test.info())
    await expect(chat.composer()).toBeVisible({ timeout: 10_000 })

    // A settled real turn gives /new something visible to clear.
    await chat.send('/new 之前的一条普通消息')
    await expect(chat.bubbles()).toHaveCount(2, { timeout: 15_000 })

    await chat.send('/new')

    // The new session is empty, the composer is reset, and the old reply is
    // gone. /new runs locally — the player's turn count is untouched.
    await expect(chat.bubbles()).toHaveCount(0, { timeout: 10_000 })
    await expect(chat.composer()).toHaveValue('')
    const snapshot = await mockSnapshot(page)
    expect(snapshot.sentTurns).toBe(1)
    expect(snapshot.sends[0]).toMatchObject({ message: '/new 之前的一条普通消息' })
    await expectNoConsoleErrors(page)
  })

  test('/cost pins the events:0 legacyHint line (the attribution caveat)', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'slash-commands', test.info())
    await expect(chat.composer()).toBeVisible({ timeout: 10_000 })

    await chat.send('/cost')
    const card = page.getByRole('status').filter({ hasText: 'Session cost' })
    await expect(card).toBeVisible({ timeout: 10_000 })
    // A pristine scripted ledger (seeded messages: [] → events 0) renders
    // the caveat — spend recorded before session attribution is not included.
    await expect(card.getByText('Usage recorded before session attribution was added is not included.')).toBeVisible()
    await page.getByRole('button', { name: 'Dismiss' }).click()
    await expectNoConsoleErrors(page)
  })

  test('parse rules at the wire: /name args and unknown /tokens reach the model as plain text', async ({ page }) => {
    test.setTimeout(90_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'slash-commands', test.info())
    await expect(chat.composer()).toBeVisible({ timeout: 10_000 })

    // `/diff now` is NOT the command — no card, the text goes out as a turn.
    await chat.send('/diff now')
    await expect(chat.bubbles()).toHaveCount(2, { timeout: 15_000 })
    await expect(page.getByRole('status').filter({ hasText: 'Git diff' })).toHaveCount(0)
    await expect(chat.bubbleAt(0)).toContainText('/diff now')

    // Unknown single tokens (pasted absolute paths) are plain text too.
    await chat.send('/usr/local/bin')
    await expect(chat.bubbles()).toHaveCount(4, { timeout: 15_000 })
    await expect(chat.bubbleAt(2)).toContainText('/usr/local/bin')
    await expect(chat.bubbleAt(3)).toContainText('未知斜杠 token 也是纯文本')

    const snapshot = await mockSnapshot(page)
    expect(snapshot.sends.map(s => s.message)).toEqual(['/diff now', '/usr/local/bin'])
    await expectNoConsoleErrors(page)
  })
})
