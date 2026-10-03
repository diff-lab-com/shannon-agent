// w3 fix/header-dropdown-hit-test — jsdom can't hit-test, so the stacking
// half of the portal fix is pinned at the token level: the header switcher
// menus ride the `z-modal` token class, and this test holds the @theme
// z-index scale to its documented ordering (index.css "z-index scale"
// block). If someone reorders the scale or drops a tier, the "floating
// layer above the chat chrome" contract the menus depend on fails loudly
// here instead of silently regressing the real UI.

import { existsSync, readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { describe, expect, it } from 'vitest'

// Same root resolution as i18nParity.test.ts: vitest's root is desktop/ui,
// so src/index.css is <cwd>/src/index.css (jsdom's import.meta.url is not a
// file URL, so it can't anchor the path).
const CSS_PATH = existsSync(resolve(process.cwd(), 'src/index.css'))
  ? resolve(process.cwd(), 'src/index.css')
  : resolve(process.cwd(), 'desktop/ui/src/index.css')

const css = readFileSync(CSS_PATH, 'utf8')

function zValue(name: string): number {
  const match = css.match(new RegExp(`--z-index-${name}:\\s*(\\d+)`))
  if (!match) throw new Error(`missing --z-index-${name} token in src/index.css`)
  return Number(match[1])
}

// The full documented scale, bottom → top.
const SCALE = ['raised', 'sticky', 'subheader', 'header', 'modal', 'scrim', 'drawer', 'flash', 'flash-above'] as const

describe('z-index token scale (stacking contract)', () => {
  it('keeps every tier strictly above the one below it', () => {
    const values = SCALE.map(zValue)
    const sorted = [...values].sort((a, b) => a - b)
    expect(sorted).toEqual(values)
    expect(new Set(values).size).toBe(values.length)
  })

  it('z-modal beats the fixed glass header and every chat-main-area tier', () => {
    // The portalled menus must paint above the header chrome (z-header,
    // where they used to be trapped) and above the message area's local
    // overlays (z-raised — ChatSearchBar, session-switch veil).
    expect(zValue('modal')).toBeGreaterThan(zValue('header'))
    expect(zValue('modal')).toBeGreaterThan(zValue('raised'))
    // ...but stay below the true modals: the permission dialog scrim and
    // the header's stop-while-waiting flash must keep winning over a menu.
    expect(zValue('modal')).toBeLessThan(zValue('scrim'))
    expect(zValue('modal')).toBeLessThan(zValue('flash'))
  })
})
