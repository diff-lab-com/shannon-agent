// Visual walkthrough + full-rules axe audit.
//
// Captures a screenshot of every main route (default + a dark theme) and runs
// a FULL axe rule set per route, writing a markdown report. Also a CI gate
// (ci.yml desktop-visual-audit) — the per-test assertion fails the suite when
// a route introduces a critical/serious violation. Run locally:
//   VITE_MOCK_MODE=1 pnpm build
//   WALKTHROUGH=1 pnpm exec playwright test -c playwright.walkthrough.config.ts
// Output: test-results/walkthrough/*.png + axe-report.md

import { test, expect } from '@playwright/test'
import AxeBuilder from '@axe-core/playwright'
import { writeFileSync, mkdirSync } from 'node:fs'

// Manual-only: skipped unless WALKTHROUGH=1 so CI's `pnpm test:e2e` skips it.
test.skip(!process.env.WALKTHROUGH, 'run manually with WALKTHROUGH=1')

const OUT_DIR = 'test-results/walkthrough'

// Main user-facing routes (experimental dev-gated surfaces excluded).
// `/timeline/<id>` renders either the timeline or its failure state — both
// are legitimate UI to audit; `/welcome` is the standalone onboarding shell
// (no sidebar).
const ROUTES: { path: string; name: string }[] = [
  { path: '/welcome', name: 'welcome' },
  { path: '/chat', name: 'chat' },
  { path: '/tasks', name: 'tasks' },
  { path: '/triage', name: 'triage' },
  { path: '/usage', name: 'usage' },
  { path: '/opc', name: 'opc' },
  { path: '/timeline/demo-session', name: 'timeline' },
  { path: '/extensions/featured', name: 'extensions-featured' },
  { path: '/extensions/mcp-servers', name: 'extensions-mcp' },
  { path: '/memory', name: 'memory' },
  { path: '/settings/general', name: 'settings-general' },
  { path: '/settings/models', name: 'settings-models' },
  { path: '/settings/theme', name: 'settings-theme' },
]

const THEMES: { id: string; label: string }[] = [
  { id: 'material', label: 'material' },
  { id: 'tokyo-night', label: 'dark-tokyo-night' },
]

const findings: string[] = []

for (const theme of THEMES) {
  for (const route of ROUTES) {
    test(`walkthrough ${route.name} [${theme.label}]`, async ({ page }) => {
      test.setTimeout(60_000)
      await page.addInitScript(t => {
        window.localStorage.setItem('shannon-theme', t as string)
      }, theme.id)
      await page.goto(route.path)
      // Shell routes settle on the sidebar; /welcome has no list items.
      await page
        .getByRole('listitem')
        .first()
        .waitFor({ state: 'visible', timeout: 5_000 })
        .catch(() => {
          /* standalone shell (welcome) — just wait out the settle delay */
        })
      await page.waitForTimeout(600) // let lazy chunks + animations settle

      mkdirSync(OUT_DIR, { recursive: true })
      await page.screenshot({ path: `${OUT_DIR}/${route.name}-${theme.label}.png`, fullPage: false })

      const results = await new AxeBuilder({ page }).analyze()
      const before = findings.length
      const bad = results.violations.filter(v => v.impact === 'critical' || v.impact === 'serious')
      for (const v of bad) {
        const targets = v.nodes.slice(0, 3).map(n => `\`${n.target.join(' ')}\``).join(' · ')
        findings.push(
          `| ${route.path} | ${theme.label} | ${v.id} | ${v.impact} | ${v.nodes.length} | ${v.help} — ${targets} |`,
        )
      }
      // Gate: this route/theme pair must not introduce critical/serious
      // violations (see axe-report.md for the diagnosable table).
      expect(findings.slice(before), `${route.path} [${theme.label}]`).toEqual([])
      // Sanity: the page rendered something.
      await expect(page.locator('body')).not.toBeEmpty()
    })
  }
}

test.afterAll(async () => {
  const report = [
    '# Full-rules axe audit (walkthrough run)',
    '',
    '| route | theme | rule | impact | nodes | help |',
    '|---|---|---|---|---|---|',
    ...findings,
    '',
    findings.length === 0
      ? 'No critical/serious violations found.'
      : `${findings.length} critical/serious violation(s) — see rows above.`,
  ].join('\n')
  writeFileSync(`${OUT_DIR}/axe-report.md`, report)
})
