import { describe, it, expect, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
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
  beforeEach(() => {
    // Reset localStorage before each test
    localStorage.clear()
    // Reset font size
    if (document.documentElement.style.fontSize) {
      document.documentElement.style.fontSize = ''
    }
  })

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

  it('renders font size section', () => {
    render(wrap(<ThemeSettings />))
    expect(screen.getByRole('heading', { name: 'Font Size' })).toBeInTheDocument()
    expect(screen.getByText('Small')).toBeInTheDocument()
    expect(screen.getByText('Medium')).toBeInTheDocument()
    expect(screen.getByText('Large')).toBeInTheDocument()
    expect(screen.getByText('X-Large')).toBeInTheDocument()
  })

  it('renders font size preview text', () => {
    render(wrap(<ThemeSettings />))
    expect(screen.getByText('The quick brown fox jumps over the lazy dog.')).toBeInTheDocument()
  })

  it('updates font scale to 1.15 when Large is clicked', () => {
    render(wrap(<ThemeSettings />))
    const largeButton = screen.getByText('Large')
    fireEvent.click(largeButton)
    expect(document.documentElement.style.fontSize).toBe('18.4px') // 16 * 1.15
  })

  it('persists font scale to localStorage', () => {
    render(wrap(<ThemeSettings />))
    const smallButton = screen.getByText('Small')
    fireEvent.click(smallButton)
    expect(localStorage.getItem('shannon.fontScale')).toBe('0.85')
  })
})
