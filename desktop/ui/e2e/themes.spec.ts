import { test, expect } from '@playwright/test'

// U9: theme switching is registry-driven. Every selectable theme must land
// its data-theme AND its scheme on <html data-theme-mode>; the light/dark
// pair must observably change a token-derived background.
const THEME_CASES: { id: string; mode: 'light' | 'dark' }[] = [
  { id: 'material', mode: 'light' },
  { id: 'tokyo-night', mode: 'dark' },
  { id: 'tokyo-night-light', mode: 'light' },
  { id: 'catppuccin', mode: 'dark' },
  { id: 'nord', mode: 'dark' },
  { id: 'ember', mode: 'dark' },
  { id: 'slate', mode: 'dark' },
  { id: 'solarized', mode: 'dark' },
  { id: 'solarized-light', mode: 'light' },
  { id: 'dracula', mode: 'dark' },
  { id: 'gruvbox', mode: 'dark' },
  { id: 'gruvbox-light', mode: 'light' },
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

test('light and dark themes observably change the surface color', async ({ page }) => {
  const bgFor = async (theme: string) => {
    await page.addInitScript(t => {
      window.localStorage.setItem('shannon-theme', t as string)
    }, theme)
    await page.goto('/')
    await page.getByRole('listitem').first().waitFor({ state: 'visible' })
    return page.evaluate(() => getComputedStyle(document.body).backgroundColor)
  }
  const light = await bgFor('material')
  const dark = await bgFor('tokyo-night')
  expect(light).toBeTruthy()
  expect(dark).toBeTruthy()
  expect(dark, 'dark surface must differ from light').not.toBe(light)
})
