// B10 — visual regression baselines for the key pages (UI audit §7 Wave 2
// acceptance: token reskin verified by diff, not by eyeball). Run
// `npx playwright test e2e/visual-baseline.spec.ts --update-snapshots`
// to re-baseline after an intentional visual change.
import { test, expect } from '@playwright/test'

const PAGES = [
  { name: 'chat', route: '/chat' },
  { name: 'tasks', route: '/tasks' },
  { name: 'settings-models', route: '/settings/models' },
]

test.describe('visual baselines', () => {
  for (const { name, route } of PAGES) {
    test(`baseline: ${name}`, async ({ page }) => {
      await page.goto(route)
      await page.waitForTimeout(2500)
      await expect(page).toHaveScreenshot(`page-${name}.png`, {
        fullPage: false,
        maxDiffPixelRatio: 0.02,
      })
    })
  }
})
