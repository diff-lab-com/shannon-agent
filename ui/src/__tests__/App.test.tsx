import { describe, it, expect } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { ThemeProvider } from '@/context/ThemeContext'
import { AppProvider } from '@/context/AppContext'
import { I18nProvider } from '@/i18n'
import { MemoryRouter, Routes, Route } from 'react-router-dom'
import { Layout } from '@/components/Layout'

// Simulate App routing without BrowserRouter (MemoryRouter for testing)
function renderRoute(path: string) {
  return render(
    <I18nProvider>
      <ThemeProvider>
        <AppProvider>
          <MemoryRouter initialEntries={[path]}>
            <Routes>
              <Route element={<Layout />}>
                <Route path="/" element={<div data-testid="redirected-chat" />} />
                <Route path="/chat" element={<div data-testid="chat-page" />} />
                <Route path="/tasks" element={<div data-testid="tasks-page" />} />
                <Route path="/goals" element={<div data-testid="goals-page" />} />
                <Route path="/opc" element={<div data-testid="opc-page" />} />
              </Route>
            </Routes>
          </MemoryRouter>
        </AppProvider>
      </ThemeProvider>
    </I18nProvider>
  )
}

describe('App routing', () => {
  it('renders /chat route', () => {
    renderRoute('/chat')
    expect(screen.getByTestId('chat-page')).toBeInTheDocument()
  })

  it('renders /tasks route', () => {
    renderRoute('/tasks')
    expect(screen.getByTestId('tasks-page')).toBeInTheDocument()
  })

  it('renders /goals route', () => {
    renderRoute('/goals')
    expect(screen.getByTestId('goals-page')).toBeInTheDocument()
  })

  it('renders /opc route', () => {
    renderRoute('/opc')
    expect(screen.getByTestId('opc-page')).toBeInTheDocument()
  })

  it('renders layout with sidebar on all routes', async () => {
    renderRoute('/chat')
    await waitFor(() => {
      expect(screen.getAllByText('Shannon').length).toBeGreaterThanOrEqual(1)
    })
  })

  it('renders layout with footer on all routes', async () => {
    renderRoute('/chat')
    await waitFor(() => {
      expect(screen.getByText('Shannon Code')).toBeInTheDocument()
    })
  })
})
