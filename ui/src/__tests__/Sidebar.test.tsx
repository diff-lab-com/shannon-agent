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

describe('Sidebar', () => {
  it('renders Shannon branding', () => {
    render(wrap(<Sidebar />))
    expect(screen.getByText('Shannon')).toBeInTheDocument()
  })

  it('renders subtitle', () => {
    render(wrap(<Sidebar />))
    expect(screen.getByText('AI Code Assistant')).toBeInTheDocument()
  })

  it('renders New Chat button', () => {
    render(wrap(<Sidebar />))
    expect(screen.getByText('New Chat')).toBeInTheDocument()
  })

  it('renders primary nav links', () => {
    render(wrap(<Sidebar />))
    expect(screen.getByText('Chat')).toBeInTheDocument()
    expect(screen.getByText('Goals')).toBeInTheDocument()
    expect(screen.getByText('Scheduled')).toBeInTheDocument()
  })

  it('renders Extensions section', () => {
    render(wrap(<Sidebar />))
    expect(screen.getByText('Extensions')).toBeInTheDocument()
  })

  it('renders OPC section', () => {
    render(wrap(<Sidebar />))
    expect(screen.getByText('OPC')).toBeInTheDocument()
  })

  it('renders Settings section', () => {
    render(wrap(<Sidebar />))
    expect(screen.getByText('Settings')).toBeInTheDocument()
  })

  it('renders extension sub-links when expanded', () => {
    render(wrap(<Sidebar />))
    expect(screen.getByText('Skills')).toBeInTheDocument()
    expect(screen.getByText('My Agents')).toBeInTheDocument()
    expect(screen.getByText('Data Sources')).toBeInTheDocument()
  })

  it('renders OPC sub-link when expanded', () => {
    render(wrap(<Sidebar />))
    expect(screen.getByText('One Person Company')).toBeInTheDocument()
  })

  it('collapses and expands Extensions section', () => {
    render(wrap(<Sidebar />))
    // Extensions is open by default
    expect(screen.getByText('Skills')).toBeInTheDocument()

    // Click Extensions button to collapse
    const extensionsButtons = screen.getAllByText('Extensions')
    fireEvent.click(extensionsButtons[0])

    // Sub-links should be gone
    expect(screen.queryByText('Skills')).not.toBeInTheDocument()

    // Click again to expand
    fireEvent.click(screen.getByText('Extensions'))
    expect(screen.getByText('Skills')).toBeInTheDocument()
  })

  it('expands Settings section on click', () => {
    render(wrap(<Sidebar />))
    // Settings sub-links are collapsed by default
    expect(screen.queryByText('General')).not.toBeInTheDocument()

    fireEvent.click(screen.getByText('Settings'))
    expect(screen.getByText('General')).toBeInTheDocument()
    expect(screen.getByText('Theme')).toBeInTheDocument()
    expect(screen.getByText('Models')).toBeInTheDocument()
    expect(screen.getByText('Usage & Billing')).toBeInTheDocument()
    expect(screen.getByText('Advanced')).toBeInTheDocument()
  })

  it('shows experiment badge on OPC', () => {
    render(wrap(<Sidebar />))
    expect(screen.getByText('Experiment')).toBeInTheDocument()
  })
})
