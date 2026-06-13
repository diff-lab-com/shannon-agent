import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { AppProvider } from '@/context/AppContext'
import { MemoryRouter, Routes, Route } from 'react-router-dom'
import { Layout } from '@/components/Layout'

function wrap(ui: React.ReactElement) {
  return (
    <AppProvider>
      <MemoryRouter initialEntries={['/chat']}>
        <Routes>
          <Route element={ui}>
            <Route path="/chat" element={<div data-testid="page">Chat</div>} />
            <Route path="/goals" element={<div data-testid="page">Goals</div>} />
          </Route>
        </Routes>
      </MemoryRouter>
    </AppProvider>
  )
}

describe('Responsive sidebar', () => {
  it('renders hamburger button', () => {
    render(wrap(<Layout />))
    expect(screen.getByLabelText('Toggle sidebar')).toBeInTheDocument()
  })

  it('renders both mobile and desktop sidebars', () => {
    render(wrap(<Layout />))
    const shannonElements = screen.getAllByText('Shannon')
    expect(shannonElements.length).toBe(2)
  })

  it('footer shows connection indicator when no usage', () => {
    render(wrap(<Layout />))
    expect(screen.getByText('Shannon Code')).toBeInTheDocument()
  })
})

describe('WelcomeState onboarding', () => {
  it('shows keyboard shortcuts in welcome screen', async () => {
    const Chat = (await import('@/pages/Chat')).default
    render(
      <AppProvider>
        <MemoryRouter initialEntries={['/chat']}>
          <Routes>
            <Route element={<Layout />}>
              <Route path="/chat" element={<Chat />} />
            </Route>
          </Routes>
        </MemoryRouter>
      </AppProvider>
    )
    await waitFor(() => {
      expect(screen.getByText(/Commands/)).toBeInTheDocument()
    })
  })
})
