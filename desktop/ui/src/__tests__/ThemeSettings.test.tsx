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
    // Reset the 材质 glass preference's DOM side effect.
    document.documentElement.classList.remove('reduce-glass')
  })

  it('renders theme subtitle', () => {
    render(wrap(<ThemeSettings />))
    // The page-level h1 was retired — the global Header shows the page
    // name; here we pin the subtitle as the page's distinctive marker.
    expect(screen.getByText(/Customize the visual environment/)).toBeInTheDocument()
  })

  it('renders theme selection section', () => {
    render(wrap(<ThemeSettings />))
    expect(screen.getByRole('heading', { name: 'Theme' })).toBeInTheDocument()
  })

  // H1 theme convergence (2026-10-17 ruling): exactly ONE user-visible
  // theme — a state card, not a picker. The card keeps the G4a real swatch
  // and the ✓ current badge, and the muted line under it states the
  // app-defined contract.
  it('renders the single Aurora Ink card with swatch + current badge and the app-defined hint', () => {
    render(wrap(<ThemeSettings />))
    const card = screen.getByTestId('theme-card-aurora-ink')
    expect(card).toBeInTheDocument()
    expect(card).toHaveAttribute('aria-current', 'true')
    expect(screen.getByTestId('theme-swatch-aurora-ink')).toBeInTheDocument()
    expect(screen.getByText('Current')).toBeInTheDocument()
    // No other theme cards exist — the dormant themes stay out of the DOM.
    expect(screen.getByTestId('theme-card-aurora-ink').parentElement!.children).toHaveLength(1)
    expect(screen.getByText('The theme is defined centrally by the app.')).toBeInTheDocument()
  })

  it('the single card is not a picker: clicking never writes a theme or flips data-theme', () => {
    localStorage.setItem('shannon-theme', 'tokyo-night') // pre-H1 leftover
    render(wrap(<ThemeSettings />))
    const card = screen.getByTestId('theme-card-aurora-ink')
    // A plain div (no button role) — the picker affordance is gone.
    expect(card.tagName).not.toBe('BUTTON')
    fireEvent.click(card)
    // Provider migration + no setTheme caller: aurora-ink both on <html> and
    // in storage, and the click cannot commit anything else.
    expect(document.documentElement.getAttribute('data-theme')).toBe('aurora-ink')
    expect(localStorage.getItem('shannon-theme')).toBe('aurora-ink')
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

  // 材质 (design-parity R1 2026-10-08): the glass-strength segmented control.
  it('renders the material card with the glass segmented control', () => {
    render(wrap(<ThemeSettings />))
    expect(screen.getByRole('heading', { name: 'Material' })).toBeInTheDocument()
    expect(screen.getByRole('radio', { name: 'Standard' })).toHaveAttribute('aria-checked', 'true')
    expect(screen.getByRole('radio', { name: 'Reduced' })).toHaveAttribute('aria-checked', 'false')
  })

  it('Reduced applies the persistent reduce-glass class and localStorage key', () => {
    render(wrap(<ThemeSettings />))
    fireEvent.click(screen.getByRole('radio', { name: 'Reduced' }))
    expect(document.documentElement.classList.contains('reduce-glass')).toBe(true)
    expect(localStorage.getItem('shannon.reduceGlass')).toBe('1')
    expect(screen.getByRole('radio', { name: 'Reduced' })).toHaveAttribute('aria-checked', 'true')
  })

  it('Standard removes the reduce-glass class and clears the localStorage key', () => {
    localStorage.setItem('shannon.reduceGlass', '1')
    document.documentElement.classList.add('reduce-glass')
    render(wrap(<ThemeSettings />))
    // Boots from the persisted preference, not the default.
    expect(screen.getByRole('radio', { name: 'Reduced' })).toHaveAttribute('aria-checked', 'true')
    fireEvent.click(screen.getByRole('radio', { name: 'Standard' }))
    expect(document.documentElement.classList.contains('reduce-glass')).toBe(false)
    expect(localStorage.getItem('shannon.reduceGlass')).toBeNull()
  })

  it('links 代码字体/终端配色 to the advanced terminal card instead of duplicating the controls', () => {
    render(wrap(<ThemeSettings />))
    const links = screen.getAllByRole('link', { name: /Adjust in Advanced/ })
    expect(links).toHaveLength(2)
    for (const link of links) expect(link).toHaveAttribute('href', '/settings/advanced')
    expect(screen.getByText('Code font')).toBeInTheDocument()
    expect(screen.getByText('Terminal colors follow theme')).toBeInTheDocument()
  })
})
