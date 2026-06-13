import { describe, it, expect } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { AppProvider } from '@/context/AppContext'
import { MemoryRouter } from 'react-router-dom'
import GeneralSettings from '@/components/settings/GeneralSettings'

function wrap(ui: React.ReactElement) {
  return (
    <AppProvider>
      <MemoryRouter>
        {ui}
      </MemoryRouter>
    </AppProvider>
  )
}

describe('GeneralSettings', () => {
  it('renders system settings heading', () => {
    render(wrap(<GeneralSettings />))
    expect(screen.getByText('System Settings')).toBeInTheDocument()
  })

  it('renders approval mode section', () => {
    render(wrap(<GeneralSettings />))
    expect(screen.getByText('Approval Mode')).toBeInTheDocument()
  })

  it('renders all approval mode options', () => {
    render(wrap(<GeneralSettings />))
    expect(screen.getAllByText('Suggest').length).toBeGreaterThanOrEqual(1)
    expect(screen.getAllByText('Confirm').length).toBeGreaterThanOrEqual(1)
    expect(screen.getAllByText('Plan').length).toBeGreaterThanOrEqual(1)
    expect(screen.getAllByText('Auto Edit').length).toBeGreaterThanOrEqual(1)
    expect(screen.getAllByText('Full Auto').length).toBeGreaterThanOrEqual(1)
  })

  it('renders provider section', () => {
    render(wrap(<GeneralSettings />))
    expect(screen.getByText('Provider')).toBeInTheDocument()
  })

  it('renders working directory label', () => {
    render(wrap(<GeneralSettings />))
    expect(screen.getByText('Working Directory')).toBeInTheDocument()
  })

  it('shows current mode description', () => {
    render(wrap(<GeneralSettings />))
    expect(screen.getByText(/Current:/)).toBeInTheDocument()
  })
})
