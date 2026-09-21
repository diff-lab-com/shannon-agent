import { describe, it, expect, beforeEach } from 'vitest'
import { render, screen } from '@testing-library/react'
import { MemoryRouter, Routes, Route } from 'react-router-dom'
import { I18nProvider } from '@/i18n'
import Settings from '@/pages/Settings'

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
      localStorage.removeItem('shannon-sidebar-mode')
    })

    it('hides the Advanced section in simple mode (default)', () => {
      renderSettings()
      expect(screen.getByText('General')).toBeInTheDocument()
      expect(screen.getByText('Theme')).toBeInTheDocument()
      expect(screen.getByText('Models')).toBeInTheDocument()
      expect(screen.queryByText('Advanced')).not.toBeInTheDocument()
    })

    it('shows the Advanced section in dev mode', () => {
      localStorage.setItem('shannon-sidebar-mode', 'dev')
      renderSettings()
      expect(screen.getByText('Advanced')).toBeInTheDocument()
    })
  })
})
