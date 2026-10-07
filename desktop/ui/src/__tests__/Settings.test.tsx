import { describe, it, expect, beforeEach } from 'vitest'
import { render, screen } from '@testing-library/react'
import { MemoryRouter, Routes, Route } from 'react-router-dom'
import { I18nProvider } from '@/i18n'
import Settings from '@/pages/Settings'
import RequireDevMode from '@/components/settings/RequireDevMode'
import { SIDEBAR_MODE_KEY } from '@/components/Sidebar'

function renderSettings() {
  return render(
    <I18nProvider>
      <MemoryRouter initialEntries={['/settings']}>
        <Routes>
          <Route path="/settings" element={<Settings />} />
        </Routes>
      </MemoryRouter>
    </I18nProvider>
  )
}

function renderAdvancedRoute() {
  return render(
    <I18nProvider>
      <MemoryRouter initialEntries={['/settings/advanced']}>
        <Routes>
          <Route path="/settings" element={<Settings />}>
            <Route path="general" element={<div data-testid="pane">general pane</div>} />
            <Route
              path="advanced"
              element={
                <RequireDevMode>
                  <div data-testid="pane">advanced pane</div>
                </RequireDevMode>
              }
            />
          </Route>
        </Routes>
      </MemoryRouter>
    </I18nProvider>
  )
}

describe('Settings', () => {
  it('renders without crashing', () => {
    const { container } = renderSettings()
    expect(container.firstChild).toBeTruthy()
  })

  it('has scrolling content area', () => {
    const { container } = renderSettings()
    const scrollable = container.querySelector('[class*="overflow-y-auto"]')
    expect(scrollable).toBeTruthy()
  })

  describe('section rail dev gate', () => {
    // Task 1 Step 4: the sidebar disclosure historically dev-gated 高级;
    // that contract now lives on the page rail (the only section switcher
    // after the dedup). Default mode is 'simple' — localStorage clean →
    // advanced stays hidden; flipping to 'dev' re-exposes it.
    beforeEach(() => {
      localStorage.removeItem(SIDEBAR_MODE_KEY)
    })

    it('hides the Advanced section in simple mode (default)', () => {
      renderSettings()
      expect(screen.getByText('General')).toBeInTheDocument()
      expect(screen.getByText('Appearance')).toBeInTheDocument()
      expect(screen.getByText('Models')).toBeInTheDocument()
      expect(screen.queryByText('Advanced')).not.toBeInTheDocument()
    })

    it('shows the Advanced section in dev mode', () => {
      localStorage.setItem(SIDEBAR_MODE_KEY, 'dev')
      renderSettings()
      expect(screen.getByText('Advanced')).toBeInTheDocument()
    })

    // IA redesign 2026-10 (ADVERSARIAL-REVIEW §2): 11 sections → 8. 网络
    // merged into 连接, 会话 into 通用, 远程目标 into 连接 — the rail shows
    // the merged set and the old entries are gone (their deep links redirect
    // in App.tsx). 8 entries in simple mode, 9 in dev.
    it('renders the merged 8 sections and no legacy entries in simple mode', () => {
      renderSettings()
      const links = screen.getAllByRole('link').map((l) => l.getAttribute('href'))
      expect(links).toEqual([
        '/settings/general',
        '/settings/theme',
        '/settings/models',
        '/settings/permissions',
        '/settings/notifications',
        '/settings/connections',
        '/settings/about',
      ])
      // The absorbed sections no longer appear on the rail.
      expect(links).not.toContain('/settings/network')
      expect(links).not.toContain('/settings/session')
      expect(links).not.toContain('/settings/remotes')
      expect(links).not.toContain('/settings/advanced')
    })

    it('renders 8 entries in dev mode with Advanced last', () => {
      localStorage.setItem(SIDEBAR_MODE_KEY, 'dev')
      renderSettings()
      const links = screen.getAllByRole('link').map((l) => l.getAttribute('href'))
      expect(links).toHaveLength(8)
      expect(links.indexOf('/settings/about')).toBeLessThan(links.indexOf('/settings/advanced'))
      expect(links[links.length - 1]).toBe('/settings/advanced')
    })
  })

  describe('advanced route guard (RequireDevMode)', () => {
    // Settings R3 (T1): the rail hides 高级 in simple mode, but deep links
    // still exist — the route itself must bounce non-dev sessions to General.
    beforeEach(() => {
      localStorage.removeItem(SIDEBAR_MODE_KEY)
    })

    it('redirects a simple-mode deep link to /settings/advanced → /settings/general', () => {
      renderAdvancedRoute()
      expect(screen.getByText('general pane')).toBeInTheDocument()
      expect(screen.queryByText('advanced pane')).not.toBeInTheDocument()
    })

    it('renders the advanced pane in dev mode', () => {
      localStorage.setItem(SIDEBAR_MODE_KEY, 'dev')
      renderAdvancedRoute()
      expect(screen.getByText('advanced pane')).toBeInTheDocument()
    })
  })
})
