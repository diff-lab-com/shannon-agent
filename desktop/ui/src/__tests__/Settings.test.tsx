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
      expect(screen.getByText('Theme')).toBeInTheDocument()
      expect(screen.getByText('Models')).toBeInTheDocument()
      expect(screen.queryByText('Advanced')).not.toBeInTheDocument()
    })

    it('shows the Advanced section in dev mode', () => {
      localStorage.setItem(SIDEBAR_MODE_KEY, 'dev')
      renderSettings()
      expect(screen.getByText('Advanced')).toBeInTheDocument()
    })

    // Settings R3 (T1): 网络 / 会话 / 关于 join the rail; 高级 moves to the
    // end and stays the only dev-gated entry — 10 entries in simple mode,
    // 11 in dev.
    it('renders the three new sections and 10 entries in simple mode', () => {
      renderSettings()
      expect(screen.getByText('Network')).toBeInTheDocument()
      expect(screen.getByText('Sessions')).toBeInTheDocument()
      expect(screen.getByText('About')).toBeInTheDocument()
      const links = screen.getAllByRole('link').map((l) => l.getAttribute('href'))
      expect(links).toHaveLength(10)
      expect(links).not.toContain('/settings/advanced')
    })

    it('renders 11 entries in dev mode with Advanced last', () => {
      localStorage.setItem(SIDEBAR_MODE_KEY, 'dev')
      renderSettings()
      const links = screen.getAllByRole('link').map((l) => l.getAttribute('href'))
      expect(links).toHaveLength(11)
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
