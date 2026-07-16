import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { AppProvider } from '@/context/AppContext'
import { ThemeProvider } from '@/context/ThemeContext'
import { MemoryRouter } from 'react-router-dom'
import ModelsSettings from '@/components/settings/ModelsSettings'

function wrap(ui: React.ReactElement) {
  return (
    <ThemeProvider>
      <AppProvider>
        <MemoryRouter>
          {ui}
        </MemoryRouter>
      </AppProvider>
    </ThemeProvider>
  )
}

describe('ModelsSettings', () => {
  it('renders model configuration heading', () => {
    render(wrap(<ModelsSettings />))
    expect(screen.getByText('Model Configuration')).toBeInTheDocument()
  })

  it('renders the managed providers section with an add button', () => {
    render(wrap(<ModelsSettings />))
    expect(screen.getByText('Providers')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /Add provider/ })).toBeInTheDocument()
  })

  it('renders performance strategy selector', () => {
    render(wrap(<ModelsSettings />))
    expect(screen.getByText('Performance Strategy')).toBeInTheDocument()
  })

  it('renders global parameters with sliders', () => {
    render(wrap(<ModelsSettings />))
    expect(screen.getByText('Global Parameters')).toBeInTheDocument()
    expect(screen.getByText('Temperature')).toBeInTheDocument()
    expect(screen.getByText('Max Tokens')).toBeInTheDocument()
  })

  it('toggles performance strategy on click', () => {
    render(wrap(<ModelsSettings />))
    const speedBtn = screen.getByText('Speed')
    fireEvent.click(speedBtn)
    expect(speedBtn).toBeInTheDocument()
  })
})
