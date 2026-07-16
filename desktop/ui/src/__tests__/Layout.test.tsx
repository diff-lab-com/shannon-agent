import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import { AppProvider } from '@/context/AppContext'
import { I18nProvider } from '@/i18n'
import { MemoryRouter, Routes, Route } from 'react-router-dom'
import { Layout } from '@/components/Layout'

function wrap(ui: React.ReactElement) {
  return (
    <I18nProvider>
      <AppProvider>
        <MemoryRouter initialEntries={['/chat']}>
          <Routes>
            <Route element={ui}>
              <Route path="/chat" element={<div data-testid="outlet-content">Chat Page Content</div>} />
            </Route>
          </Routes>
        </MemoryRouter>
      </AppProvider>
    </I18nProvider>
  )
}

describe('Layout', () => {
  it('renders sidebar', () => {
    render(wrap(<Layout />))
    expect(screen.getAllByText('Shannon').length).toBeGreaterThanOrEqual(1)
  })

  it('renders outlet content', () => {
    render(wrap(<Layout />))
    expect(screen.getByTestId('outlet-content')).toBeInTheDocument()
  })

  it('renders footer', () => {
    render(wrap(<Layout />))
    // Footer shows "Shannon Code" when no usage/status
    expect(screen.getByText('Shannon Code')).toBeInTheDocument()
  })
})
