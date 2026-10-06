/**
 * §4 P0-A — document-level link interceptor: external http(s) links are
 * routed through openLink, `#anchor` and `/app` routes stay native, and
 * unknown relative links are blocked so the webview never navigates away.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const openLinkMock = vi.fn()

vi.mock('@/lib/openLink', async (importOriginal) => {
  const actual = await importOriginal()
  return { ...actual, openLink: (...args: unknown[]) => openLinkMock(...args) }
})

import { classifyHref, installLinkInterception, resetLinkInterceptionForTests } from '@/lib/linkInterceptor'

function clickAnchor(href: string, init: Partial<MouseEvent> = {}): MouseEvent {
  const a = document.createElement('a')
  a.setAttribute('href', href)
  document.body.appendChild(a)
  const e = new MouseEvent('click', { bubbles: true, cancelable: true, button: 0, ...init })
  a.dispatchEvent(e)
  a.remove()
  return e
}

describe('classifyHref', () => {
  it('buckets hrefs into the four action classes', () => {
    expect(classifyHref('https://example.com')).toBe('external-http')
    expect(classifyHref('http://localhost:1420')).toBe('external-http')
    expect(classifyHref('#fn-1')).toBe('anchor')
    expect(classifyHref('/chat')).toBe('app')
    expect(classifyHref('foo.md')).toBe('other')
    expect(classifyHref('mailto:a@b.c')).toBe('other')
  })
})

describe('installLinkInterception', () => {
  beforeEach(() => {
    openLinkMock.mockReset()
    resetLinkInterceptionForTests()
  })

  afterEach(() => {
    resetLinkInterceptionForTests()
  })

  it('intercepts external links and routes them through openLink', () => {
    installLinkInterception()
    const e = clickAnchor('https://example.com/docs')
    expect(e.defaultPrevented).toBe(true)
    expect(openLinkMock).toHaveBeenCalledWith('https://example.com/docs', undefined)
  })

  it('sends Alt+click to the browser override', () => {
    installLinkInterception()
    clickAnchor('https://example.com/x', { altKey: true })
    expect(openLinkMock).toHaveBeenCalledWith('https://example.com/x', 'browser')
  })

  it('leaves anchors and app routes untouched', () => {
    installLinkInterception()
    const anchor = clickAnchor('#fn-1')
    const route = clickAnchor('/chat')
    expect(anchor.defaultPrevented).toBe(false)
    expect(route.defaultPrevented).toBe(false)
    expect(openLinkMock).not.toHaveBeenCalled()
  })

  it('blocks unknown relative links without calling openLink', () => {
    installLinkInterception()
    const e = clickAnchor('foo.md')
    expect(e.defaultPrevented).toBe(true)
    expect(openLinkMock).not.toHaveBeenCalled()
  })

  it('is idempotent — a second install must not double-route', () => {
    installLinkInterception()
    installLinkInterception()
    clickAnchor('https://example.com/once')
    expect(openLinkMock).toHaveBeenCalledTimes(1)
  })
})
