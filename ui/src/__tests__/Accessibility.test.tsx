import { describe, it, expect } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { AppProvider } from '@/context/AppContext'
import { MemoryRouter } from 'react-router-dom'
import { Sidebar } from '@/components/Sidebar'

function wrap(ui: React.ReactElement, { path = '/chat' } = {}) {
  return (
    <AppProvider>
      <MemoryRouter initialEntries={[path]}>
        {ui}
      </MemoryRouter>
    </AppProvider>
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
      expect(screen.getByText('Projects')).toBeInTheDocument()
      expect(screen.getByText('Scheduled')).toBeInTheDocument()
    })

    it('settings sub-nav items have visible text when expanded', () => {
      render(wrap(<Sidebar />))
      fireEvent.click(screen.getByText('Settings'))
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
