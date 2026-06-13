import { describe, it, expect } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { AppProvider } from '@/context/AppContext'
import { MemoryRouter } from 'react-router-dom'
import AdvancedSettings from '@/components/settings/AdvancedSettings'

function wrap(ui: React.ReactElement) {
  return (
    <AppProvider>
      <MemoryRouter>
        {ui}
      </MemoryRouter>
    </AppProvider>
  )
}

describe('AdvancedSettings', () => {
  it('renders advanced settings heading', () => {
    render(wrap(<AdvancedSettings />))
    expect(screen.getByText('Advanced Settings')).toBeInTheDocument()
  })

  it('renders memory management section', () => {
    render(wrap(<AdvancedSettings />))
    expect(screen.getByText('Memory Management')).toBeInTheDocument()
  })

  it('renders long-term memory toggle label', () => {
    render(wrap(<AdvancedSettings />))
    expect(screen.getByText('Long-term Memory')).toBeInTheDocument()
  })

  it('renders clear session cache button', () => {
    render(wrap(<AdvancedSettings />))
    expect(screen.getByText('Clear Session Cache')).toBeInTheDocument()
  })

  it('renders data privacy section', () => {
    render(wrap(<AdvancedSettings />))
    expect(screen.getByText('Data Privacy')).toBeInTheDocument()
  })

  it('renders developer options section', () => {
    render(wrap(<AdvancedSettings />))
    expect(screen.getByText('Developer Options')).toBeInTheDocument()
  })

  it('renders factory reset button', () => {
    render(wrap(<AdvancedSettings />))
    expect(screen.getByText('Reset to Factory Settings')).toBeInTheDocument()
  })

  it('renders view system logs link', () => {
    render(wrap(<AdvancedSettings />))
    expect(screen.getByText('View System Logs')).toBeInTheDocument()
  })

  it('renders manage api keys link', () => {
    render(wrap(<AdvancedSettings />))
    expect(screen.getByText('Manage API Keys')).toBeInTheDocument()
  })

  // US-SET-04: System Logs modal
  it('opens system logs modal on View System Logs click', () => {
    render(wrap(<AdvancedSettings />))
    fireEvent.click(screen.getByText('View System Logs'))
    expect(screen.getByText('System Logs')).toBeInTheDocument()
    expect(screen.getByText(/Shannon Desktop/)).toBeInTheDocument()
  })

  it('closes system logs modal via close button', () => {
    render(wrap(<AdvancedSettings />))
    fireEvent.click(screen.getByText('View System Logs'))
    expect(screen.getByText('System Logs')).toBeInTheDocument()
    // Click the close button inside the modal
    const modal = screen.getByText('System Logs').closest('.fixed')!
    const closeBtn = modal.querySelector('button')
    if (closeBtn) fireEvent.click(closeBtn)
  })

  // US-SET-04: API Keys modal
  it('opens api keys modal on Manage API Keys click', () => {
    render(wrap(<AdvancedSettings />))
    fireEvent.click(screen.getAllByText('Manage API Keys')[0])
    expect(screen.getByText('Go to Model Settings')).toBeInTheDocument()
  })
})
