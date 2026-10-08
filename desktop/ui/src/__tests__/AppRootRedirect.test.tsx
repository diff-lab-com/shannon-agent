// W10 audit §6-F — the `/` redirect keeps the query string alive.
//
// A session window boots on `/?windowSession=<uuid>`. The redirect used to
// be `<Navigate to="/chat" replace />`, which resolves with an EMPTY search
// — the windowSession param was stripped from the URL, so a reload
// (F5/Ctrl+R) parsed a null windowSession and the same window degraded into
// full main-window form (sidebar back, pin lost).
//
// The reload is simulated at the same layer the webview reload reads:
// whatever search the router URL carries after the redirect is what
// `parseWindowSession` sees on the next boot.

import { describe, it, expect } from 'vitest'
import { render } from '@testing-library/react'
import { MemoryRouter, Routes, Route, useLocation } from 'react-router-dom'
import { RootRedirect } from '@/App'
import { parseWindowSession } from '@/lib/windowSession'

const UUID_A = '7e6c3f18-4a2e-4f6a-9a52-6d1c1a0f83f1'

function renderRootRedirect(initialEntry: string) {
  let landed: { pathname: string; search: string } | null = null
  function LandedProbe() {
    const location = useLocation()
    landed = { pathname: location.pathname, search: location.search }
    return null
  }
  render(
    <MemoryRouter initialEntries={[initialEntry]}>
      <Routes>
        <Route path="/" element={<RootRedirect />} />
        <Route path="/chat" element={<LandedProbe />} />
      </Routes>
    </MemoryRouter>,
  )
  return () => landed
}

describe('W10 §6-F — / redirect preserves the windowSession query', () => {
  it('lands on /chat with the windowSession param intact (F5 keeps window mode)', () => {
    const readLanded = renderRootRedirect(`/?windowSession=${UUID_A}`)

    const landed = readLanded()
    expect(landed?.pathname).toBe('/chat')
    expect(landed?.search).toBe(`?windowSession=${UUID_A}`)
    // The post-redirect URL is what a reload parses — window mode holds.
    expect(parseWindowSession(landed?.search)).toBe(UUID_A)
  })

  it('a main-window boot (bare /) still lands on /chat with no search', () => {
    const readLanded = renderRootRedirect('/')
    const landed = readLanded()
    expect(landed?.pathname).toBe('/chat')
    expect(landed?.search).toBe('')
    expect(parseWindowSession(landed?.search)).toBeNull()
  })

  it('a malformed windowSession param is still carried (parse stays the gatekeeper)', () => {
    const readLanded = renderRootRedirect('/?windowSession=garbage')
    const landed = readLanded()
    expect(landed?.pathname).toBe('/chat')
    expect(landed?.search).toBe('?windowSession=garbage')
    // The parser (not the redirect) rejects it — a malformed param must
    // never silently switch sessions.
    expect(parseWindowSession(landed?.search)).toBeNull()
  })
})
