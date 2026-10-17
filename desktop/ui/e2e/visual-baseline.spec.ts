// B10 — visual regression baselines for the main routes (UI audit §7 Wave 2
// acceptance: token reskin verified by diff, not by eyeball). Run
// `npx playwright test e2e/visual-baseline.spec.ts --update-snapshots`
// to re-baseline after an intentional visual change.
//
// H1 batch (2026-10-17): expanded from 3 anchor pages to the full main-route
// set (13 pages) — every user-facing shell page gets a token-level
// regression guard, not just chat/tasks/settings-models. Deliberately NOT
// baselined:
//   - /opc, /timeline/<id>, /companion, /settings/{advanced,permissions,...}
//     — dev-gated, parameterized, or window-mode surfaces covered by other
//     specs (walkthrough/axe keeps auditing them; adding them here would
//     double-maintain unstable frames),
//   - /welcome redirects: none observed in mock mode (get_config returns a
//     provider, so Layout.tsx's no-provider bounce never fires and the
//     standalone shell renders directly — same posture walkthrough.spec and
//     welcome-provider.spec rely on).
import { test, expect } from '@playwright/test'

const PAGES = [
  { name: 'chat', route: '/chat' },
  { name: 'tasks', route: '/tasks' },
  { name: 'settings-models', route: '/settings/models' },
  { name: 'welcome', route: '/welcome' },
  { name: 'usage', route: '/usage' },
  { name: 'files', route: '/files' },
  { name: 'triage', route: '/triage' },
  { name: 'memory', route: '/memory' },
  { name: 'extensions-featured', route: '/extensions/featured' },
  { name: 'extensions-mcp-servers', route: '/extensions/mcp-servers' },
  { name: 'settings-general', route: '/settings/general' },
  { name: 'settings-connections', route: '/settings/connections' },
  { name: 'settings-theme', route: '/settings/theme' },
]

test.describe('visual baselines', () => {
  for (const { name, route } of PAGES) {
    test(`baseline: ${name}`, async ({ page }) => {
      await page.goto(route)
      // H1: every route must still be THE route after the boot settles — a
      // surprise bounce (e.g. a Layout gate firing) would silently re-baseline
      // the wrong page.
      expect(page.url(), `${route} must not bounce`).toContain(route)
      await page.waitForTimeout(2500)
      await expect(page).toHaveScreenshot(`page-${name}.png`, {
        fullPage: false,
        // 5% headroom for Linux-Chromium sub-pixel rendering noise (CI
        // runners ship slightly different font metrics than macOS dev boxes).
        // Snapshots are token-level regression guard, not pixel-perfect art.
        maxDiffPixelRatio: 0.05,
      })
    })
  }
})
