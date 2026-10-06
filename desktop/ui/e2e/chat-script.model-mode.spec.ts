// W2 journey #17（§9.4，gap G3+G4）— model-mode-switch: the composer model
// chip's session override arc (suffix → Set as default → Reset), the effort
// label, the ExecutionModeSwitcher's approval_mode writes with the honest
// rawLabel echo for engine-only values, the PhaseTierSwitcher's tier write,
// the "a switch takes effect on the NEXT turn" semantics pinned through the
// player's send-time `model` stamp, and G4 — the strict mode still surfaces
// the permission dialog for the same tool.
//
// Selectors: the Task3 branch (feat/chat-testids-registry) owns testids for
// these chrome surfaces; each anchor below prefers the testid CONSTANT and
// falls back to the same element's stable role/aria label, so the spec runs
// green both before and after that branch folds into dev.
import { expect, test, type Page } from '@playwright/test'

import { ChatPage } from './helpers/ChatPage'
import { expectMockPhase, loadChatScript, loadChatScriptObject, readChatScript } from './helpers/scriptLoader'
import { expectNoConsoleErrors } from './helpers/watchdog'
import { mockSnapshot } from './helpers/knownIssues'
import type { ChatScript } from '../src/lib/mock/scripted/schema'

const script = readChatScript('model-mode-switch') as ChatScript

const SEEDED_MODEL = 'claude-haiku-4-5-20251001'
const SWITCH_MODEL = 'gpt-5'

// testid lands in feat/chat-testids-registry (model-chip-trigger)
const MODEL_CHIP_TESTID = 'model-chip-trigger'
// testid lands in feat/chat-testids-registry (approval-mode-pill)
const APPROVAL_PILL_TESTID = 'approval-mode-pill'
// testid lands in feat/chat-testids-registry (execution-mode-switcher)
const EXEC_MODE_TESTID = 'execution-mode-switcher'
// testid lands in feat/chat-testids-registry (permission-dialog)
const PERMISSION_DIALOG_TESTID = 'permission-dialog'

/** The composer model chip (Select trigger). */
function modelChip(page: Page) {
  return page.getByTestId(MODEL_CHIP_TESTID).or(page.getByRole('combobox', { name: 'Model' }))
}
/** The composer approval-mode pill (Select trigger). */
function approvalPill(page: Page) {
  return page.getByTestId(APPROVAL_PILL_TESTID).or(page.getByRole('combobox', { name: 'Permission mode' }))
}
/** The header execution-mode switcher trigger. */
function execModeTrigger(page: Page) {
  return page.getByTestId(EXEC_MODE_TESTID).or(page.getByRole('button', { name: /^Execution mode:/ }))
}
/** The Header permission alertdialog. */
function permissionDialog(page: Page) {
  return page.getByTestId(PERMISSION_DIALOG_TESTID).or(page.getByRole('alertdialog'))
}

/** Read the desktop config through the same invoke route the app uses. */
function getConfig(page: Page): Promise<Record<string, unknown>> {
  return page.evaluate(async () => {
    return (window as unknown as {
      __TAURI_INTERNALS__: { invoke(cmd: string, args?: Record<string, unknown>): Promise<Record<string, unknown>> }
    }).__TAURI_INTERNALS__.invoke('get_config', {})
  })
}

/** Open the seeded session so the chip is session-scoped and sends carry an id. */
async function openSession(page: Page): Promise<void> {
  await page.getByTestId('desktop-session-row-script-sess-model').click()
  await expect(page.getByRole('heading', { name: 'Model switch' })).toBeVisible({ timeout: 10_000 })
}

test.describe('scripted chat backend — model-mode-switch (journey #17)', () => {
  test('seeded approvalMode echoes rawLabel; the override arc lands on the next turn (sends.model)', async ({ page }) => {
    test.setTimeout(90_000)
    const chat = new ChatPage(page)
    // The rawLabel half needs a seeded engine-only approval_mode; the shared
    // YAML stays clean (the ExecutionModeSwitcher test below arms the plain
    // seed), so this arm mutates the object in memory — same schema, same
    // ajv gate.
    const rawLabelScript = {
      ...script,
      seed: { ...script.seed, config: { ...script.seed?.config, approvalMode: 'bypass_permissions' } },
    }
    await loadChatScriptObject(page, rawLabelScript, test.info())
    await openSession(page)

    // Engine-only value: the pill echoes it VERBATIM (rawLabel) instead of
    // masquerading as a listed tier — the honest-fallback contract.
    await expect(approvalPill(page)).toContainText('Bypass approvals')

    // The chip reflects the seeded session override: catalog name + the
    // never-silent "· session" suffix.
    const chip = modelChip(page)
    await expect(chip).toContainText('Claude Haiku 4.5')
    await expect(chip).toContainText('session')

    // Turn 1 — the send-time model stamp carries the seeded override.
    await chat.send(script.turns[0]!.user)
    await expect(chat.sendButton()).toBeVisible({ timeout: 15_000 })
    expect((await mockSnapshot(page)).sends[0]).toMatchObject({ turnIndex: 0, model: SEEDED_MODEL })

    // Chip switch → session-scoped write, suffix stays.
    await chip.click()
    // S1-2 (P-N2): the session section spells out the pin contract — a
    // pinned session does not get model-level automatic failover.
    await expect(page.getByText(/do not fail over automatically/i)).toBeVisible()
    // S3-1 (P-N11): the picker states the precedence chain at the top, and
    // the SEEDED override row wears the "Session override active" why label
    // (Claude-Code-style "why is this model active" tags).
    await expect(page.getByTestId('model-picker-priority-line')).toBeVisible()
    await expect(page.getByTestId('why-badge-session')).toBeVisible()
    await page.getByTestId(`model-option-${SWITCH_MODEL}`).click()
    await expect(chip).toContainText('GPT-5')
    await expect(chip).toContainText('session')
    await expect.poll(async () => (await getConfig(page)).approval_mode).toBe('bypass_permissions')

    // Effort pick — S3-5: the effort sub-tier expands under the EFFECTIVE
    // model's row (the just-pinned GPT-5); picking one commits via the
    // effort: menu value and wears the effort BADGE beside the chip — the
    // `name · Deep` label glue is gone (P2-19). The chip keeps only its
    // `· session` suffix.
    await chip.click()
    await page.getByRole('option', { name: 'Deep' }).click()
    await expect(page.getByTestId('effort-badge')).toContainText('Deep')
    await expect(chip).not.toContainText('Deep')

    // "Set as default" promotes the session pick to the engine-global
    // default (configure('model')) and toasts it.
    await chip.click()
    await page.getByTestId('model-action-set-default').click()
    await expect(page.getByText('Default model set to GPT-5')).toBeVisible({ timeout: 5_000 })
    await expect.poll(async () => (await getConfig(page)).model).toBe(SWITCH_MODEL)

    // Turn 2 — the chip switch from BEFORE the send takes effect NOW:
    // sends[1].model is gpt-5 (next-turn semantics, the journey's core).
    // The turn parks at its permission-request; allow it and release the
    // scripted waitFor so the run settles before the next leg.
    await chat.send(script.turns[1]!.user)
    await expectMockPhase(page, 'waitingPermission', 10_000)
    const dialog = permissionDialog(page)
    await expect(dialog).toBeVisible({ timeout: 5_000 })
    await dialog.getByRole('button', { name: 'Allow Once' }).click()
    await expect
      .poll(async () => (await mockSnapshot(page)).permissionLog, { timeout: 5_000 })
      .toEqual([expect.objectContaining({ requestId: 'pr-model-1', allow: true })])
    await expect(dialog).toHaveCount(0)
    await page.evaluate(() => {
      (window as unknown as {
        __shannonMock: { control: { resume(): void } }
      }).__shannonMock.control.resume()
    })
    expect((await mockSnapshot(page)).sends[1]).toMatchObject({ turnIndex: 1, model: SWITCH_MODEL })
    await expect(chat.sendButton()).toBeVisible({ timeout: 15_000 })

    // Reset to default: the override is dropped — the chip re-inherits the
    // PROMOTED global default (GPT-5, from "Set as default" above) and keeps
    // the global effort tier: the model name comes back bare on the chip
    // while the effort badge (S3-5) carries the "Deep" state.
    // The bare not.toContainText('session') would also pass on a wrong-model
    // or placeholder chip, so pin the positive label too (next send observes
    // the inheritance as model: null).
    await chip.click()
    await page.getByTestId('model-action-clear-override').click()
    await expect(chip).toContainText('GPT-5', { timeout: 5_000 })
    await expect(page.getByTestId('effort-badge')).toContainText('Deep', { timeout: 5_000 })
    await expect(chip).not.toContainText('session', { timeout: 5_000 })

    await chat.send(script.turns[2]!.user)
    await expect(chat.sendButton()).toBeVisible({ timeout: 15_000 })
    expect((await mockSnapshot(page)).sends[2]).toMatchObject({ turnIndex: 2, model: null })
    await expect(chat.bubbles()).toHaveCount(6)

    await expectNoConsoleErrors(page)
  })

  test('ExecutionModeSwitcher writes approval_mode per tier, PhaseTierSwitcher writes act_tier, and Strict still surfaces the dialog (G4)', async ({ page }) => {
    test.setTimeout(90_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'model-mode-switch', test.info())
    await openSession(page)

    // Turn 1 first: the empty conversation renders the WelcomeState overlay
    // over the message area, which would swallow the header dropdowns; the
    // settled run also matches the journey narrative (confirm, then switch).
    await chat.send(script.turns[0]!.user)
    await expect(chat.sendButton()).toBeVisible({ timeout: 15_000 })
    expect((await mockSnapshot(page)).sends[0]).toMatchObject({ turnIndex: 0, model: SEEDED_MODEL })

    // Demo default (no seeded approvalMode): the config ships the engine's
    // `standard` alias, which the pill echoes verbatim (rawLabel) — the
    // quick-switch table's labels only appear once a listed tier is picked.
    await expect(approvalPill(page)).toContainText('standard')

    // Permissive tier: activate_permission_profile('permissive') → the mock
    // mirrors the backend mapping (auto_edit) — the pill re-renders. The
    // pick goes through a REAL pointer click: the menu renders in a
    // body-level portal at the z-modal token, so it wins the hit-test over
    // the message area (w3 fix/header-dropdown-hit-test — the old
    // dispatchEvent('click') workaround is retired; reverting the portal
    // must turn this spec red).
    const execMenu = () => page.getByRole('listbox', { name: 'Execution mode options' })
    const pickTier = async (name: string) => {
      await execModeTrigger(page).click()
      const option = execMenu().getByRole('option', { name })
      await option.waitFor({ timeout: 5_000 })
      await option.click()
      await execMenu().waitFor({ timeout: 5_000 }).catch(() => {}) // menu closes on pick
    }
    await pickTier('Permissive')
    await expect(page.getByText('Execution mode: Permissive')).toBeVisible({ timeout: 5_000 })
    await expect(execModeTrigger(page)).toContainText('Permissive')
    await expect(approvalPill(page)).toContainText('Auto Edit')
    await expect.poll(async () => (await getConfig(page)).approval_mode).toBe('auto-edit')

    // Strict tier: back to suggest engine-side (the four-tier table's
    // Balanced entry), the switcher label follows.
    await pickTier('Strict')
    await expect(page.getByText('Execution mode: Strict')).toBeVisible({ timeout: 5_000 })
    await expect(execModeTrigger(page)).toContainText('Strict')
    await expect(approvalPill(page)).toContainText(/ask/i)
    await expect.poll(async () => (await getConfig(page)).approval_mode).toBe('ask')

    // PhaseTierSwitcher (testid already exists): Act → Fast writes the
    // global act_tier the backend resolves on the next query. Same real
    // pointer click as the tier switcher — the popover is portal-mounted at
    // the z-modal token, so the hit-test reaches the radio.
    await page.getByTestId('phase-tier-switcher').click()
    const actFast = page.getByTestId('phase-tier-menu').getByRole('radiogroup', { name: 'Execution tier' }).getByRole('radio', { name: 'Fast' })
    await actFast.waitFor({ timeout: 5_000 })
    await actFast.click()
    await expect(page.getByText('Execution tier: Fast')).toBeVisible({ timeout: 5_000 })
    await expect(page.getByTestId('phase-tier-switcher')).toContainText('Fast')
    await expect.poll(async () => (await getConfig(page)).act_tier).toBe('fast')

    // G4 — mode decides the dialog: under Strict the SAME tool still (and
    // always) surfaces the permission prompt. The seeded override also
    // stamps sends[1].model without any UI switch (the seed-only path).
    await chat.send(script.turns[1]!.user)
    await expectMockPhase(page, 'waitingPermission', 10_000)
    const dialog = permissionDialog(page)
    await expect(dialog).toBeVisible({ timeout: 5_000 })
    await expect(dialog.getByText('rm -rf build')).toBeVisible()
    await dialog.getByRole('button', { name: 'Allow Once' }).click()
    await expect
      .poll(async () => (await mockSnapshot(page)).permissionLog, { timeout: 5_000 })
      .toEqual([expect.objectContaining({ requestId: 'pr-model-1', allow: true })])
    await expect(page.locator('[data-tool-name="Bash"][data-tool-status="error"]')).toBeVisible({ timeout: 10_000 })
    expect((await mockSnapshot(page)).sends[1]).toMatchObject({ turnIndex: 1, model: SEEDED_MODEL })

    await page.evaluate(() => {
      (window as unknown as {
        __shannonMock: { control: { resume(): void } }
      }).__shannonMock.control.resume()
    })
    await expect(chat.sendButton()).toBeVisible({ timeout: 15_000 })
    await expect(chat.bubbles()).toHaveCount(4)
    await expectNoConsoleErrors(page)
  })
})
