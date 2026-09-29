// G6 (UI review 2026-09-29): runtime guard for the glass performance budget.
//
// index.css documents a hard budget — ≤4 backdrop-filter elements per screen
// (sidebar rail excluded by design: it sits over the solid --material-base
// with nothing to blur, so it deliberately carries no backdrop-filter).
// Until now that budget lived only in a comment; this spec counts VISIBLE
// elements whose computed style carries a backdrop-filter on the four main
// surfaces and fails when a change pushes a page over budget.
//
// Counted: getComputedStyle(el).backdropFilter !== 'none' AND the element is
// rendered (nonzero box, visible, opacity > 0). Closed popovers/drawers
// (width 0, display:none, opacity:0) don't count.
//
// Budget rationale: the persistent chrome trio is Sidebar(0, solid base) +
// Header(glass-surface) + Footer(glass-surface); the 4th slot belongs to at
// most one more surface (chat composer, extensions toolbar, an open dock or
// dropdown). 4 is the designed ceiling, not a guess.

import { test, expect } from '@playwright/test'

const ROUTES = ['/chat', '/tasks', '/settings/general', '/extensions/featured']
const BUDGET = 4

test.describe('glass budget (≤4 backdrop-filter elements per screen)', () => {
  for (const route of ROUTES) {
    test(`${route} stays within the budget`, async ({ page }) => {
      await page.goto(route)
      // The shell is ready when the theme machinery has landed its attribute
      // and the sidebar rail is rendered — same settle contract as themes.spec.
      await expect(page.locator('html')).toHaveAttribute('data-theme', /.+/)
      await page.getByRole('listitem').first().waitFor({ state: 'visible' })
      // Let lazy route chunks + panel-in entrances settle so transient
      // animation frames can't be counted.
      await page.waitForTimeout(600)

      const { count, details } = await page.evaluate(() => {
        const hits: string[] = []
        for (const el of document.querySelectorAll('*')) {
          const cs = getComputedStyle(el)
          const bf = cs.backdropFilter || cs.getPropertyValue('-webkit-backdrop-filter')
          if (!bf || bf === 'none') continue
          const rect = el.getBoundingClientRect()
          if (rect.width <= 0 || rect.height <= 0) continue
          if (cs.visibility === 'hidden' || cs.display === 'none') continue
          if (Number.parseFloat(cs.opacity || '1') === 0) continue
          const cls = (typeof el.className === 'string' ? el.className : '').trim()
          hits.push(`<${el.tagName.toLowerCase()}${cls ? ` class="${cls.slice(0, 90)}"` : ''}>`)
        }
        return { count: hits.length, details: hits }
      })

      // Keep the live number visible in CI logs — the budget is only useful
      // while we can see how close each surface actually runs to it.
      console.log(`[glass-budget] ${route}: ${count}/${BUDGET} → ${details.join(' ') || '(none)'}`)

      // Attach the offending elements to the assertion so a regression names
      // the component that brought the 5th backdrop layer.
      expect(
        count,
        `${route} renders ${count} backdrop-filter elements (budget ${BUDGET}):\n${details.join('\n')}`,
      ).toBeLessThanOrEqual(BUDGET)
    })
  }
})
