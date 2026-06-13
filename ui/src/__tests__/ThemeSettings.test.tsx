import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import { AppProvider } from '@/context/AppContext'
import { ThemeProvider } from '@/context/ThemeContext'
import { MemoryRouter } from 'react-router-dom'
import ThemeSettings from '@/components/settings/ThemeSettings'

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

describe('ThemeSettings', () => {
  it('renders theme settings heading', () => {
    render(wrap(<ThemeSettings />))
    expect(screen.getByText('Theme Settings')).toBeInTheDocument()
  })

  it('renders theme selection section', () => {
    render(wrap(<ThemeSettings />))
    expect(screen.getByRole('heading', { name: 'Theme' })).toBeInTheDocument()
  })

  it('renders active theme section', () => {
    render(wrap(<ThemeSettings />))
    expect(screen.getByText('Active Theme')).toBeInTheDocument()
  })

  it('renders color swatches', () => {
    render(wrap(<ThemeSettings />))
    expect(screen.getByTitle('Primary')).toBeInTheDocument()
    expect(screen.getByTitle('Secondary')).toBeInTheDocument()
    expect(screen.getByTitle('Tertiary')).toBeInTheDocument()
  })
})
