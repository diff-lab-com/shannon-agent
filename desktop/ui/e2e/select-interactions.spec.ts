// Select interaction guards (fix/select-commit) — lock in real commits for
// the highest-traffic dropdowns: composer permission mode, reasoning effort,
// and the model chip ↔ header sync. These cover the regression window where
// the demo mock's `configure` was a silent no-op (wire-shape mismatch), which
// had made every dropdown LOOK broken. Mock note: `configure` persists into
// demoConfig, and `get_status` mirrors it — so a successful commit is
// visible in both selectors.
//
// UI notes (模型名去重 / audit D8 / GB round-1 R3):
//  - the composer placeholder is context-aware, so waits target the
//    textarea's stable aria-label ("Message") instead of placeholder text;
//  - reasoning effort is FOLDED into the model chip's dropdown as a
//    namespaced section — there is no separate Reasoning combobox;
//  - the permission dropdown offers the FOUR shared tiers
//    (Ask/Auto Edit/Full) — `plan` is owned by the composer's
//    plan toggle and `confirm` is an out-of-table engine value, so neither
//    is a dropdown option;
//  - the Header model selector is hidden on /chat (the composer chip is the
//    single surface there), so chip→header sync is verified cross-page.
import { test, expect } from '@playwright/test'

test.describe('Select interactions', () => {
  test('permission mode commits from the composer dropdown', async ({ page }) => {
    await page.goto('/chat')
    await page.getByRole('textbox', { name: 'Message' }).waitFor({ timeout: 15000 })
    await page.waitForTimeout(800)

    const trigger = page.getByRole('combobox', { name: 'Permission mode' })
    await trigger.click()
    // 4+3 model: the shared three-ladder table (the demo mock's legacy
    // 'standard' value shows as the raw readout with nothing selected).
    // Auto Edit writes `auto-edit` — a REAL state change the demo
    // configure persists.
    await page.getByRole('option', { name: 'Auto Edit' }).click()
    await expect(trigger).toContainText(/auto edit/i, { timeout: 10000 })
  })

  test('reasoning effort commits from the model chip dropdown', async ({ page }) => {
    await page.goto('/chat')
    await page.getByRole('textbox', { name: 'Message' }).waitFor({ timeout: 15000 })
    await page.waitForTimeout(800)

    // Effort entries live in the model chip's dropdown under the
    // "Reasoning effort" section; picking one re-renders the chip as
    // "<model> · <effort label>".
    const trigger = page.getByRole('combobox', { name: 'Model' })
    await trigger.click()
    await page.getByRole('option', { name: 'Deep' }).click()
    await expect(trigger).toContainText(/·\s*Deep/i, { timeout: 10000 })
  })

  test('model chip switch is session-scoped; the global default stays untouched (R2-1)', async ({ page }) => {
    await page.goto('/chat')
    await page.getByRole('textbox', { name: 'Message' }).waitFor({ timeout: 15000 })
    await page.waitForTimeout(800)

    const chip = page.getByRole('combobox', { name: 'Model' })
    await chip.click()
    // R2-3: model rows carry context/price meta, so locators use the stable
    // per-model test id instead of the accessible name.
    await page.getByTestId('model-option-gpt-5').click()
    // The chip reflects the SESSION override, never silently: "· session".
    await expect(chip).toContainText('GPT-5', { timeout: 10000 })
    await expect(chip).toContainText('session', { timeout: 10000 })
    // The Header selector (hidden on /chat) still shows the GLOBAL default —
    // R2-1 kept the chip switch session-local. Navigate via the SPA link — a
    // full page reload resets the mock backend's in-memory state.
    await page.getByRole('link', { name: 'Settings' }).click()
    await expect(page.getByRole('button', { name: 'Select model' })).toContainText(
      /claude-sonnet-4-6/i,
      { timeout: 10000 },
    )
  })

  test('"Set as default" in the chip menu promotes the session pick to the global default (R2-1)', async ({ page }) => {
    await page.goto('/chat')
    await page.getByRole('textbox', { name: 'Message' }).waitFor({ timeout: 15000 })
    await page.waitForTimeout(800)

    const chip = page.getByRole('combobox', { name: 'Model' })
    await chip.click()
    await page.getByTestId('model-option-gpt-5').click()
    await expect(chip).toContainText('GPT-5', { timeout: 10000 })

    // The menu's "Set as default" performs the pre-R2-1 global write.
    await chip.click()
    await page.getByTestId('model-action-set-default').click()
    await page.getByRole('link', { name: 'Settings' }).click()
    // Demo get_status mirrors the demo config, so the promoted model id
    // (gpt-5, the canonical catalog id) shows on the non-chat header.
    await expect(page.getByRole('button', { name: 'Select model' })).toContainText(
      /gpt-5/i,
      { timeout: 10000 },
    )
  })
})
