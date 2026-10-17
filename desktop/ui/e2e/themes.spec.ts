import { test, expect } from '@playwright/test'
import AxeBuilder from '@axe-core/playwright'

// U9 → H1: theme switching is registry-driven, and since the H1 convergence
// (2026-10-17 ruling) Aurora Ink is the ONLY user-visible theme. The spec
// keeps its original shape narrowed to that single entry: the theme must
// land its data-theme AND its registered scheme on <html data-theme-mode>;
// the token-derived surface must observably match the registry.
//
// The 12 sibling themes remain in theme-source.json / the generated registry
// as DORMANT INFRASTRUCTURE (UI-unreachable) — they are deliberately NOT
// swept here anymore; re-importing one means re-adding its case.
const THEME_CASES: { id: string; mode: 'light' | 'dark' }[] = [
  { id: 'aurora-ink', mode: 'dark' },
]

test('every theme sets data-theme and the registered scheme on <html>', async ({ page }) => {
  for (const { id, mode } of THEME_CASES) {
    await page.addInitScript(t => {
      window.localStorage.setItem('shannon-theme', t as string)
    }, id)
    await page.goto('/')
    await page.getByRole('listitem').first().waitFor({ state: 'visible' })
    expect(await page.getAttribute('html', 'data-theme'), id).toBe(id)
    expect(await page.getAttribute('html', 'data-theme-mode'), id).toBe(mode)
  }
})

// H1 migration: any non-aurora-ink stored value — dormant theme picks and
// the retired 'system' mode alike — normalizes to aurora-ink on boot and is
// written back, so the pre-H1 localStorage values can never surface a
// different look.
test('stored pre-H1 theme values (tokyo-night, material, system) migrate to aurora-ink', async ({ page }) => {
  for (const stored of ['tokyo-night', 'material', 'system']) {
    await page.addInitScript(t => {
      window.localStorage.setItem('shannon-theme', t as string)
    }, stored)
    await page.goto('/')
    await page.getByRole('listitem').first().waitFor({ state: 'visible' })
    expect(await page.getAttribute('html', 'data-theme'), stored).toBe('aurora-ink')
    expect(await page.getAttribute('html', 'data-theme-mode'), stored).toBe('dark')
    // The migration commits: the storage key now carries aurora-ink.
    expect(await page.evaluate(() => window.localStorage.getItem('shannon-theme'))).toBe('aurora-ink')
  }
})

// Average sRGB channel of an "rgb(r, g, b)" string, normalized to 0..1.
// Authored light surfaces sit ≈0.9+ and dark surfaces ≈0.2-, so 0.5 splits
// them cleanly without per-theme thresholds.
function surfaceLuminance(rgb: string): number {
  const m = /rgba?\((\d+),\s*(\d+),\s*(\d+)/.exec(rgb)
  if (!m) throw new Error(`unexpected background color: ${rgb}`)
  return (Number(m[1]) + Number(m[2]) + Number(m[3])) / (3 * 255)
}

// The registry (ThemeContext THEME_SCHEMES) must describe the theme's actual
// token block, or the `dark:` variants fire on light surfaces (this caught
// ember/slate being registered 'dark' over light token blocks). H1: the
// single shipped theme must render its registered dark surface — the old
// light↔dark pair assertion lost its light leg with the retired themes.
test('every theme’s registered scheme matches its actual surface luminance', async ({ page }) => {
  for (const { id, mode } of THEME_CASES) {
    await page.addInitScript(t => {
      window.localStorage.setItem('shannon-theme', t as string)
    }, id)
    await page.goto('/')
    await page.getByRole('listitem').first().waitFor({ state: 'visible' })
    const bg = await page.evaluate(() => getComputedStyle(document.body).backgroundColor)
    const avg = surfaceLuminance(bg)
    if (mode === 'light') {
      expect(avg, `${id} is registered light but its surface (${bg}) is dark`).toBeGreaterThan(0.5)
    } else {
      expect(avg, `${id} is registered dark but its surface (${bg}) is light`).toBeLessThan(0.5)
    }
  }
})

// Color-contrast is the one a11y rule whose outcome depends on the active
// theme, so it needs a per-theme sweep rather than the single-theme axe run
// in sidebar-sessions.spec.ts. Text tokens were fixed to AA under
// scripts/contrast-audit.mjs; this guards the rendered result on every theme.
test('every theme passes axe color-contrast on the chat page', async ({ page }) => {
  test.setTimeout(180_000)
  for (const { id } of THEME_CASES) {
    await page.addInitScript(t => {
      window.localStorage.setItem('shannon-theme', t as string)
    }, id)
    await page.goto('/')
    await page.getByRole('listitem').first().waitFor({ state: 'visible' })
    // axe reads PAINTED text: wait until the theme attribute is applied and
    // one painted frame has elapsed, so a busy runner can't catch a
    // mid-repaint state (seen once as a flake in the full suite).
    await page.waitForFunction(
      t => document.documentElement.getAttribute('data-theme') === t,
      id,
    )
    await page.evaluate(() => new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r))))
    // Kill color transitions before axe reads painted text: `transition-colors`
    // lingers ~150ms after the theme swap, and on a busy runner the double-rAF
    // above still lands mid-transition (seen as tokyo-night flakes locally).
    await page.addStyleTag({
      content: '*, *::before, *::after { transition: none !important; animation: none !important; }',
    })
    const results = await new AxeBuilder({ page })
      .withRules(['color-contrast'])
      .analyze()
    const bad = results.violations.filter(v => v.impact === 'critical' || v.impact === 'serious')
    // Attach computed colors of the offending elements to the failure message —
    // a raw selector alone doesn't say which token pair broke.
    const detail = await Promise.all(bad.flatMap(v => v.nodes.slice(0, 3).map(async n => {
      const sel = n.target.join(' ')
      return page.evaluate(s => {
        const el = document.querySelector(s)
        if (!el) return `${s} — not found`
        const cs = getComputedStyle(el)
        let bg = 'transparent'
        for (let p = el; p && p !== document.documentElement; p = p.parentElement) {
          const c = getComputedStyle(p).backgroundColor
          if (c && c !== 'rgba(0, 0, 0, 0)') { bg = c; break }
        }
        return `${s} — color: ${cs.color} on ${bg}, text: "${(el.textContent ?? '').trim().slice(0, 40)}"`
      }, sel)
    })))
    expect(
      bad.map(v => ({ id: v.id, nodes: v.nodes.slice(0, 3).map(n => n.target) })),
      `${id}: contrast violations\n${detail.join('\n')}`,
    ).toEqual([])
  }
})
