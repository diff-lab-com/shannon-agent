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
//  - reasoning effort is the SELECTED model row's expanded sub-tier inside
//    the model chip's dropdown (S3-5, 裁定⑪) — no separate Reasoning
//    combobox, no bottom section;
//  - the permission control is a FOUR-stop segmented control
//    (Ask/Auto Edit/Plan/Full — Aurora 2026-10, 裁决 B1); below the 1200px
//    breakpoint it folds back into the current-mode chip dropdown;
//  - the Header model selector is hidden on /chat (the composer chip is the
//    single surface there), so chip→header sync is verified cross-page.
import { test, expect } from '@playwright/test'

test.describe('Select interactions', () => {
  test('permission mode commits from the composer segmented control', async ({ page }) => {
    await page.goto('/chat')
    await page.getByRole('textbox', { name: 'Message' }).waitFor({ timeout: 15000 })
    await page.waitForTimeout(800)

    // Aurora 2026-10 (裁决 B1): the approval ladder is a four-stop segmented
    // control at ≥1200px (ask / auto-edit / plan / full-auto). Clicking Auto
    // Edit writes `auto-edit` — a REAL state change the demo configure
    // persists, so the controlled aria-checked flips. A seeded out-of-ladder
    // value (e.g. 'standard') renders through the raw badge with nothing
    // selected — covered by chat-script.model-mode.
    const group = page.getByTestId('approval-mode-pill')
    await expect(group).toBeVisible()
    await expect(group.getByRole('radio')).toHaveCount(4)
    await group.getByTestId('approval-mode-segment-auto-edit').click()
    await expect(group.getByTestId('approval-mode-segment-auto-edit')).toHaveAttribute('aria-checked', 'true', { timeout: 10000 })
  })

  test('reasoning effort commits from the model chip dropdown', async ({ page }) => {
    await page.goto('/chat')
    await page.getByRole('textbox', { name: 'Message' }).waitFor({ timeout: 15000 })
    await page.waitForTimeout(800)

    // S3-5 (裁定⑪): the effort entries are the SELECTED model row's
    // expanded sub-tier — picking one re-renders the effort BADGE beside
    // the chip; the chip label itself stays the bare model name (the old
    // `name · Deep` glue is gone, P2-19).
    const trigger = page.getByRole('combobox', { name: 'Model' })
    await trigger.click()
    await expect(page.getByTestId('effort-subtier-header')).toBeVisible()
    await page.getByRole('option', { name: 'Deep' }).click()
    await expect(page.getByTestId('effort-badge')).toContainText(/Deep/i, { timeout: 10000 })
    await expect(trigger).not.toContainText(/Deep/i, { timeout: 10000 })

    // Re-opening the picker shows the sub-tier again (expanded under the
    // same effective model row).
    await trigger.click()
    await expect(page.getByTestId('effort-option-high')).toBeVisible()
    await page.keyboard.press('Escape')
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
