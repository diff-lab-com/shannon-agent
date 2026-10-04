// P2-4 / R9-④ — render-layer gate for remote images in chat markdown
// (docs/plans/2026-10-05-chat-r3-improvement-plan.md §三 P2-4, §六 B3-4):
// `![](https://…?d=<context>)` must not fire a request on render. Matrix:
// default-blocked placeholder (no network surface) → per-image admit →
// global allow switch (default off); local sources never affected.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { act, fireEvent, render, screen } from '@testing-library/react'
import { Markdown } from '@/components/chat/Markdown'
import {
  isGatedRemoteImageSrc,
  remoteImageHost,
  setRemoteImagesAllowed,
  isRemoteImagesAllowed,
} from '@/lib/remoteImages'

const REMOTE_MD = '![remote cat](https://example.com/cat.png?d=secret)'

function renderMd(md: string) {
  return render(<Markdown>{md}</Markdown>)
}

beforeEach(() => {
  // The store is module-global; pin it back to the shipped default so tests
  // don't leak the allow state into each other (also wipes the localStorage
  // key whenever a previous test turned the switch on).
  setRemoteImagesAllowed(false)
})

describe('remoteImages — src classification', () => {
  it('gates absolute and scheme-relative http(s) sources', () => {
    expect(isGatedRemoteImageSrc('https://example.com/cat.png')).toBe(true)
    expect(isGatedRemoteImageSrc('http://example.com/cat.png')).toBe(true)
    // Scheme-relative resolves under the page protocol — still a third party.
    expect(isGatedRemoteImageSrc('//evil.example/pixel.gif?d=x')).toBe(true)
    expect(isGatedRemoteImageSrc('  https://spaces.example/x.png  ')).toBe(true)
  })

  it('never gates local or self-contained sources', () => {
    expect(isGatedRemoteImageSrc('/home/u/pic.png')).toBe(false)
    expect(isGatedRemoteImageSrc('file:///home/u/pic.png')).toBe(false)
    expect(isGatedRemoteImageSrc('asset://localhost/pic.png')).toBe(false)
    // B2 P2-16: the asset protocol's Linux shape is an http:// URL — it
    // serves local files, so the host whitelist must exempt it.
    expect(isGatedRemoteImageSrc('http://asset.localhost/pic.png')).toBe(false)
    expect(isGatedRemoteImageSrc('https://asset.localhost/pic.png')).toBe(false)
    expect(isGatedRemoteImageSrc('data:image/png;base64,AAAA')).toBe(false)
    expect(isGatedRemoteImageSrc('blob:https://example.com/uuid')).toBe(false)
    expect(isGatedRemoteImageSrc('images/pic.png')).toBe(false)
    expect(isGatedRemoteImageSrc('')).toBe(false)
  })

  it('parses the display host out of the URL', () => {
    expect(remoteImageHost('https://img.example.com/a/b.png?d=x')).toBe('img.example.com')
    expect(remoteImageHost('//host.example/x.png')).toBe('host.example')
  })
})

describe('Markdown remote-image gate (P2-4)', () => {
  it('renders a placeholder with host + alt and NO img element by default', () => {
    const { container } = renderMd(REMOTE_MD)
    // No network surface: the img element itself is absent from the DOM.
    expect(screen.queryByAltText('remote cat')).not.toBeInTheDocument()
    expect(container.querySelector('img')).toBeNull()
    const gate = screen.getByTestId('remote-image-gate')
    expect(gate).toHaveTextContent('example.com')
    expect(gate).toHaveTextContent('remote cat')
  })

  it('falls back to the URL filename when the image has no alt', () => {
    renderMd('![](https://example.com/a/pic.png)')
    expect(screen.getByTestId('remote-image-gate')).toHaveTextContent('pic.png')
  })

  it('admits exactly one image per button click, restoring the plain lazy img', () => {
    renderMd(`${REMOTE_MD}\n\n![other](https://example.com/other.png)`)
    const buttons = screen.getAllByRole('button', { name: /example\.com/ })
    expect(buttons).toHaveLength(2)
    fireEvent.click(buttons[0])
    // Only the clicked src is admitted; the sibling stays gated.
    const img = screen.getByAltText('remote cat')
    expect(img).toHaveAttribute('src', 'https://example.com/cat.png?d=secret')
    expect(img.getAttribute('loading')).toBe('lazy')
    expect(screen.queryByAltText('other')).not.toBeInTheDocument()
    expect(screen.getAllByTestId('remote-image-gate')).toHaveLength(1)
  })

  it('exposes a keyboard-operable allow control (a real button)', () => {
    renderMd(REMOTE_MD)
    const btn = screen.getByRole('button', { name: 'Load remote image from example.com' })
    expect(btn.tagName).toBe('BUTTON')
    fireEvent.click(btn)
    expect(screen.getByAltText('remote cat')).toBeInTheDocument()
  })

  it('loads remote images directly when the global switch is on', () => {
    setRemoteImagesAllowed(true)
    renderMd(REMOTE_MD)
    expect(screen.getByAltText('remote cat')).toHaveAttribute('src', 'https://example.com/cat.png?d=secret')
    expect(screen.queryByTestId('remote-image-gate')).toBeNull()
  })

  it('re-gates already-rendered images when the switch flips back off (store subscription crosses the memo boundary)', () => {
    setRemoteImagesAllowed(true)
    renderMd(REMOTE_MD)
    expect(screen.getByAltText('remote cat')).toBeInTheDocument()
    act(() => setRemoteImagesAllowed(false))
    expect(screen.queryByAltText('remote cat')).not.toBeInTheDocument()
    expect(screen.getByTestId('remote-image-gate')).toBeInTheDocument()
  })

  it('leaves local sources untouched (abs path, asset protocol); file:// stays sanitize-stripped as before', () => {
    renderMd('![a](/some/file.png)\n\n![b](file:///some/other.png)\n\n![c](http://asset.localhost/some/file.png)')
    // convertFileSrc is mocked to the asset:// shape in test setup.
    expect(screen.getByAltText('a')).toHaveAttribute('src', 'asset://localhost/some/file.png')
    // file:// was ALREADY stripped by the GitHub sanitize schema (src
    // protocols: http/https + relative only) — the gate doesn't change that
    // pre-existing inertness (no src attribute, no request).
    expect(screen.getByAltText('b')).not.toHaveAttribute('src')
    expect(screen.getByAltText('c')).toHaveAttribute('src', 'http://asset.localhost/some/file.png')
    expect(screen.queryByTestId('remote-image-gate')).toBeNull()
  })

  it('keeps the admitted remote image non-interactive (no dock open)', () => {
    const seen = vi.fn()
    window.addEventListener('shannon:open-artifact-file', seen)
    renderMd(REMOTE_MD)
    fireEvent.click(screen.getByRole('button', { name: /example\.com/ }))
    fireEvent.click(screen.getByAltText('remote cat'))
    window.removeEventListener('shannon:open-artifact-file', seen)
    expect(seen).not.toHaveBeenCalled()
  })
})

describe('remoteImages — persistence (frontend-local)', () => {
  it('writes the switch to localStorage and re-reads it in a fresh module instance', async () => {
    setRemoteImagesAllowed(true)
    expect(localStorage.getItem('shannon.chat.allowRemoteImages')).toBe('1')
    expect(isRemoteImagesAllowed()).toBe(true)
    vi.resetModules()
    const fresh = await import('@/lib/remoteImages')
    expect(fresh.isRemoteImagesAllowed()).toBe(true)
    fresh.setRemoteImagesAllowed(false)
    expect(localStorage.getItem('shannon.chat.allowRemoteImages')).toBe('0')
    vi.resetModules()
    const fresh2 = await import('@/lib/remoteImages')
    expect(fresh2.isRemoteImagesAllowed()).toBe(false)
  })

  it('defaults to blocked when storage holds anything but "1"', async () => {
    localStorage.setItem('shannon.chat.allowRemoteImages', 'yes')
    vi.resetModules()
    const fresh = await import('@/lib/remoteImages')
    expect(fresh.isRemoteImagesAllowed()).toBe(false)
  })
})
