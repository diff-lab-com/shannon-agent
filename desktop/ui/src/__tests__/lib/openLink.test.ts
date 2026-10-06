/**
 * §4 P0-A — the openLink pipeline: target resolution, panel router
 * registration/degradation, and the browser fallback via open_external.
 *
 * Decision §5-6: default target is `panel`; before the web-tab host
 * registers a router, `panel` degrades to the system browser.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const openExternalMock = vi.fn()

vi.mock('@/lib/tauri-api', () => ({
  openExternal: (...args: unknown[]) => openExternalMock(...args),
}))

vi.mock('sonner', () => ({
  toast: { error: vi.fn() },
}))

import {
  getLinkTarget,
  openLink,
  registerLinkPanelRouter,
  resetLinkPanelRouterForTests,
  setLinkTarget,
} from '@/lib/openLink'

describe('openLink', () => {
  beforeEach(() => {
    openExternalMock.mockReset()
    openExternalMock.mockResolvedValue(undefined)
    localStorage.clear()
    resetLinkPanelRouterForTests()
  })

  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('defaults to panel target', () => {
    expect(getLinkTarget()).toBe('panel')
  })

  it('persists an explicit target and reads it back', () => {
    setLinkTarget('browser')
    expect(getLinkTarget()).toBe('browser')
    setLinkTarget('panel')
    expect(getLinkTarget()).toBe('panel')
  })

  it('routes panel-target links through the registered panel router without invoking Rust', async () => {
    const router = vi.fn()
    registerLinkPanelRouter(router)
    await openLink('https://example.com/a')
    expect(router).toHaveBeenCalledWith('https://example.com/a')
    expect(openExternalMock).not.toHaveBeenCalled()
  })

  it('degrades panel target to the system browser when no router is registered', async () => {
    await openLink('https://example.com/b')
    expect(openExternalMock).toHaveBeenCalledWith('https://example.com/b')
  })

  it('override=browser always opens in the system browser even with a router', async () => {
    const router = vi.fn()
    registerLinkPanelRouter(router)
    await openLink('https://example.com/c', 'browser')
    expect(router).not.toHaveBeenCalled()
    expect(openExternalMock).toHaveBeenCalledWith('https://example.com/c')
  })

  it('override=panel uses the router regardless of the stored setting', async () => {
    setLinkTarget('browser')
    const router = vi.fn()
    registerLinkPanelRouter(router)
    await openLink('https://example.com/d', 'panel')
    expect(router).toHaveBeenCalledWith('https://example.com/d')
    expect(openExternalMock).not.toHaveBeenCalled()
  })

  it('keeps the stored browser target when a router exists', async () => {
    setLinkTarget('browser')
    const router = vi.fn()
    registerLinkPanelRouter(router)
    await openLink('https://example.com/e')
    expect(router).not.toHaveBeenCalled()
    expect(openExternalMock).toHaveBeenCalledWith('https://example.com/e')
  })
})
