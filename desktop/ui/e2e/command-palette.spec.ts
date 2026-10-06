import { test, expect } from '@playwright/test'

/**
 * Command Palette coverage — re-enabled by the 2026-08-26 audit fix.
 *
 * History: R5 originally skipped this spec. CommandPalette crashed on
 * open ("Cannot read properties of undefined (reading 'subscribe')")
 * because CommandDialog rendered the cmdk children without the
 * <Command> root, so every store-subscribing child hit an undefined
 * context. Fixed in ui/src/components/ui/command.tsx; these tests now
 * lock the open/close contract and a keyboard navigation round-trip
 * against a real Chromium.
 */
test.describe('Command palette', () => {
  // The mod+k handler attaches in a Layout effect one commit after first
  // paint — under a loaded runner a raw networkidle keypress can land before
  // it and be swallowed. Anchor the keypress on a mounted surface instead.
  async function waitAppInteractive(page: import('@playwright/test').Page): Promise<void> {
    await expect(page.getByRole('textbox', { name: 'Message' })).toBeVisible()
  }

  test('Ctrl+K opens the palette and Escape closes it', async ({ page }) => {
    await page.goto('/')
    await page.waitForLoadState('networkidle')
    await waitAppInteractive(page)
    await page.keyboard.press('Control+k')
    const dialog = page.getByRole('dialog')
    await expect(dialog).toBeVisible()

    await page.keyboard.press('Escape')
    await expect(dialog).toBeHidden()
  })

  test('filter + Enter navigates to the matched page command', async ({ page }) => {
    await page.goto('/')
    await page.waitForLoadState('networkidle')
    await waitAppInteractive(page)
    await page.keyboard.press('Control+k')
    const dialog = page.getByRole('dialog')
    await expect(dialog).toBeVisible()

    // "theme" uniquely matches the Theme Settings page command — a pure
    // route navigation, no side effects. (The old "billing" probe was
    // removed with the billing page itself, P0-4 decision D5.)
    await page.keyboard.type('theme')
    await page.keyboard.press('Enter')
    await expect(page).toHaveURL(/settings\/theme/)
  })
})
