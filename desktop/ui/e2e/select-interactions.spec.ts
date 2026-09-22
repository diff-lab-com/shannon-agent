// Select interaction guards (fix/select-commit) — lock in real commits for
// the highest-traffic dropdowns: composer permission mode, reasoning effort,
// and the model chip ↔ header sync. These cover the regression window where
// the demo mock's `configure` was a silent no-op (wire-shape mismatch), which
// had made every dropdown LOOK broken. Mock note: `configure` persists into
// demoConfig, and `get_status` mirrors it — so a successful commit is
// visible in both selectors.
//
// UI notes (模型名去重 / audit D8):
//  - the composer placeholder is context-aware, so waits target the
//    textarea's stable aria-label ("Message") instead of placeholder text;
//  - reasoning effort is FOLDED into the model chip's dropdown as a
//    namespaced section — there is no separate Reasoning combobox;
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
    await page.getByRole('option', { name: 'Plan' }).click()
    // The selected item's label renders lowercase ('plan') — case-insensitive.
    await expect(trigger).toContainText(/plan/i, { timeout: 10000 })
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

  test('model chip commit syncs the header selector on non-chat pages', async ({ page }) => {
    await page.goto('/chat')
    await page.getByRole('textbox', { name: 'Message' }).waitFor({ timeout: 15000 })
    await page.waitForTimeout(800)

    const chip = page.getByRole('combobox', { name: 'Model' })
    await chip.click()
    await page.getByRole('option', { name: 'GPT-5', exact: true }).click()
    await expect(chip).toContainText('GPT-5', { timeout: 10000 })
    // Both surfaces write the same config keys; the Header selector (hidden
    // on /chat) reflects the commit on the next page. Navigate via the SPA
    // link — a full page reload would reset the mock backend's in-memory
    // demoConfig and lose the commit.
    await page.getByRole('link', { name: 'Settings' }).click()
    await expect(page.getByRole('button', { name: 'Select model' })).toContainText(
      'GPT-5',
      { timeout: 10000 },
    )
  })
})
