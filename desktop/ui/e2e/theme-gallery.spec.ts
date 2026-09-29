// Theme gallery re-shoot for docs/design/ui-audit-2026-09/THEME-GALLERY.md.
//
// One viewport screenshot of /chat per theme, written as chat-<theme>.png into
// GALLERY_OUT_DIR (the doc's screenshots/themes/ directory when driven by the
// script). Manual-only: the theme list comes in via GALLERY_THEMES (comma
// separated ids), so the default `pnpm exec playwright test` run registers
// nothing here and CI is unaffected. Normally driven by:
//   pnpm gallery:shoot
//   (scripts/shoot-theme-gallery.mjs — reads src/theme/generated/registry.ts,
//   shoots the subset, regenerates the markdown table)

import { test, expect } from '@playwright/test'
import { mkdirSync } from 'node:fs'

const OUT_DIR = process.env.GALLERY_OUT_DIR ?? 'test-results/theme-gallery'
const THEMES = (process.env.GALLERY_THEMES ?? '')
  .split(',')
  .map(t => t.trim())
  .filter(Boolean)

// Manual-only: no GALLERY_THEMES → no tests registered → default runs skip
// this file entirely (same gating idea as walkthrough.spec.ts's WALKTHROUGH).
test.skip(THEMES.length === 0, 'run manually via `pnpm gallery:shoot` (sets GALLERY_THEMES)')

// The gallery doc frames every theme at 1280×800 (glass composer + sidebar +
// message flow visible); /chat is a viewport-locked app shell, so a viewport
// shot IS the full page. Locale is pinned to zh-CN so re-shoots are
// reproducible across machines (the app follows navigator.language, and
// Playwright's default en-US would flip the committed zh gallery to English).
test.use({ viewport: { width: 1280, height: 800 }, locale: 'zh-CN' })

for (const theme of THEMES) {
  test(`gallery /chat [${theme}]`, async ({ page }) => {
    test.setTimeout(60_000)
    await page.addInitScript(t => {
      window.localStorage.setItem('shannon-theme', t as string)
    }, theme)
    await page.goto('/chat')
    await page.getByRole('listitem').first().waitFor({ state: 'visible', timeout: 5_000 })
    // Guard against shooting a theme that silently fell back to the default.
    expect(await page.getAttribute('html', 'data-theme'), theme).toBe(theme)
    await page.waitForTimeout(600) // let lazy chunks + animations settle

    mkdirSync(OUT_DIR, { recursive: true })
    await page.screenshot({ path: `${OUT_DIR}/chat-${theme}.png`, fullPage: false })
    // Sanity: the page rendered something.
    await expect(page.locator('body')).not.toBeEmpty()
  })
}
