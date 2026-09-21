import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import { AppProvider } from '@/context/AppContext'
import { I18nProvider } from '@/i18n'
import { MemoryRouter, Routes, Route, Navigate } from 'react-router-dom'
import { Sidebar } from '@/components/Sidebar'
import Settings from '@/pages/Settings'

function wrap(ui: React.ReactElement, { path = '/chat' } = {}) {
  return (
    <I18nProvider>
      <AppProvider>
        <MemoryRouter initialEntries={[path]}>
          {ui}
        </MemoryRouter>
      </AppProvider>
    </I18nProvider>
  )
}

describe('Accessibility', () => {
  describe('Sidebar', () => {
    it('decorative icons are hidden from screen readers', () => {
      const { container } = render(wrap(<Sidebar />))
      const hiddenIcons = container.querySelectorAll('[aria-hidden="true"]')
      expect(hiddenIcons.length).toBeGreaterThanOrEqual(1)
    })

    it('New Chat button has visible text label', () => {
      render(wrap(<Sidebar />))
      expect(screen.getByText('New Chat')).toBeInTheDocument()
    })

    it('nav links have visible text labels', () => {
      render(wrap(<Sidebar />))
      expect(screen.getByText('Chat')).toBeInTheDocument()
      expect(screen.getByText('Tasks')).toBeInTheDocument()
    })

    it('settings section nav is visible on the Settings page rail', () => {
      render(
        wrap(
          <Routes>
            <Route path="/settings" element={<Settings />}>
              <Route index element={<Navigate to="general" replace />} />
              <Route path="general" element={null} />
            </Route>
          </Routes>,
          { path: '/settings/general' },
        ),
      )
      expect(screen.getByText('General')).toBeInTheDocument()
      expect(screen.getByText('Theme')).toBeInTheDocument()
      expect(screen.getByText('Models')).toBeInTheDocument()
    })
  })

  describe('Focus management', () => {
    it('interactive elements are buttons not spans', () => {
      const { container } = render(wrap(<Sidebar />))
      const buttons = container.querySelectorAll('button')
      expect(buttons.length).toBeGreaterThan(0)
    })
  })
})
