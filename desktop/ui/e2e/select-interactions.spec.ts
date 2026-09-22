// Select interaction guards (fix/select-commit) — lock in real commits for
// the highest-traffic dropdowns: composer permission mode, reasoning effort,
// and the model chip ↔ header sync. These cover the regression window where
// the demo mock's `configure` was a silent no-op (wire-shape mismatch), which
// had made every dropdown LOOK broken. Mock note: `configure` persists into
// demoConfig, and `get_status` mirrors it — so a successful commit is
// visible in both selectors.
import { test, expect } from '@playwright/test'

test.describe('Select interactions', () => {
  test('permission mode commits from the composer dropdown', async ({ page }) => {
    await page.goto('/chat')
    await page.getByPlaceholder(/Ask Shannon anything.../i).waitFor({ timeout: 15000 })
    await page.waitForTimeout(800)

    const trigger = page.getByRole('combobox', { name: 'Permission mode' })
    await trigger.click()
    await page.getByRole('option', { name: 'Plan' }).click()
    // The selected item's label renders lowercase ('plan') — case-insensitive.
    await expect(trigger).toContainText(/plan/i, { timeout: 10000 })
  })

  test('reasoning effort commits from the composer dropdown', async ({ page }) => {
    await page.goto('/chat')
    await page.getByPlaceholder(/Ask Shannon anything.../i).waitFor({ timeout: 15000 })
    await page.waitForTimeout(800)

    const trigger = page.getByRole('combobox', { name: 'Reasoning' })
    await trigger.click()
    // en locale: low/medium/high/max are labelled Basic/Standard/Deep/Ultra.
    await page.getByRole('option', { name: 'Deep' }).click()
    // Base UI's Value renders the stored value ('high'), not the item label.
    await expect(trigger).toContainText(/high/i, { timeout: 10000 })
  })

  test('model chip and header selector commit and stay in sync', async ({ page }) => {
    await page.goto('/chat')
    await page.getByPlaceholder(/Ask Shannon anything.../i).waitFor({ timeout: 15000 })
    await page.waitForTimeout(800)

    const chip = page.getByRole('combobox', { name: 'Model' })
    await chip.click()
    await page.getByRole('option', { name: 'GPT-5', exact: true }).click()
    await expect(chip).toContainText('GPT-5', { timeout: 10000 })
    await expect(page.getByRole('button', { name: 'Select model' })).toContainText('GPT-5')
  })
})
