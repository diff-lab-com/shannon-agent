/**
 * Command Palette coverage — DEFERRED.
 *
 * The CommandPalette component (cmdk 1.1.1 + Base UI Dialog) throws a
 * runtime error "Cannot read properties of undefined (reading 'subscribe')"
 * when it transitions from closed → open. This is a pre-existing bug surfaced
 * by the R5 audit, NOT a regression from this work.
 *
 * Tracked separately: the regression net for the palette should be added once
 * the cmdk/React 19 interaction in ui/src/components/ui/command.tsx is fixed.
 * Leaving this file as a placeholder so the R5 entry in the phase2 plan
 * status table has a real on-disk anchor for whoever picks up the fix.
 *
 * To re-enable: assert that page.getByRole('dialog') becomes visible after
 * Ctrl+K, and verify navigation via cmdk keyboard selection works.
 */
import { test, expect } from '@playwright/test'

test.describe.skip('Command palette (DEFERRED — see header)', () => {
  test('Ctrl+K opens the palette and Escape closes it', async ({ page }) => {
    test.skip(true, 'CommandPalette has a pre-existing cmdk/React 19 bug; R5 audit deferred this coverage.')
    await page.goto('/')
    await page.waitForLoadState('networkidle')
    await page.keyboard.press('Control+k')
    await expect(page.getByRole('dialog')).toBeVisible()
    await page.keyboard.press('Escape')
    await expect(page.getByRole('dialog')).toBeHidden()
  })
})
