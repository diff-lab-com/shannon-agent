import { test, expect } from '@playwright/test'

/**
 * Modal regression net — Escape close + focus restoration.
 *
 * Runs in playwright.config.ts mock mode (`pnpm demo` via webServer).
 * Locks R1b's behavioral contract: Modal is built on a controlled open +
 * onClose prop. The internal implementation can swap to Base UI Dialog
 * (R1b) without changing what users observe, as long as these tests pass.
 *
 * Target: AdvancedSettings → "Clear Session Cache" ConfirmDialog.
 * It's reachable purely from mock-mode routes (no Tauri command beyond
 * configure('clear_cache')).
 *
 * Backdrop-click close was intentionally omitted: it's flaky in headless
 * Chromium against fixed-overlay divs that use the e.target === e.currentTarget
 * idiom (the matching tested-modal-stack standard) and adds little value over
 * the Escape and focus-restoration contracts covered below.
 */
test.describe('Modal interactions (R5 regression net)', () => {
  test('Escape closes a Modal opened from AdvancedSettings', async ({ page }) => {
    await page.goto('/settings/advanced')
    await page.waitForLoadState('networkidle')

    const trigger = page.getByRole('button', { name: /Clear Session Cache/i })
    await expect(trigger).toBeVisible()
    await trigger.click()

    // The ConfirmDialog wraps Modal which renders role="alertdialog"
    const dialog = page.getByRole('alertdialog')
    await expect(dialog).toBeVisible()

    await page.keyboard.press('Escape')
    await expect(dialog).toBeHidden()
  })

  test('triggering button regains focus after Modal closes', async ({ page }) => {
    await page.goto('/settings/advanced')
    await page.waitForLoadState('networkidle')

    const trigger = page.getByRole('button', { name: /Clear Session Cache/i })
    await trigger.click()

    const dialog = page.getByRole('alertdialog')
    await expect(dialog).toBeVisible()

    // Close via Escape (works regardless of focus position).
    await page.keyboard.press('Escape')
    await expect(dialog).toBeHidden()

    // Focus restoration contract: focus returns to the trigger button.
    // The Base UI Modal pattern implements this by default; preserving
    // it is the "behavior parity" guarantee for R1b.
    await expect(trigger).toBeFocused()
  })
})
