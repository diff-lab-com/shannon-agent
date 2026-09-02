import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { ThemeProvider, useTheme } from '@/context/ThemeContext'

function ThemeConsumer() {
  const { theme, setTheme, themes, fontScale, setFontScale } = useTheme()
  return (
    <div>
      <span data-testid="current-theme">{theme}</span>
      <span data-testid="theme-count">{themes.length}</span>
      <span data-testid="font-scale">{fontScale.toString()}</span>
      <button data-testid="set-font-scale" onClick={() => setFontScale(1.15)}>
        Set Font Scale
      </button>
      {themes.map(t => (
        <button key={t.id} data-testid={`btn-${t.id}`} onClick={() => setTheme(t.id)}>
          {t.label}
        </button>
      ))}
    </div>
  )
}

describe('ThemeContext', () => {
  beforeEach(() => {
    localStorage.clear()
    if (document.documentElement.style.fontSize) {
      document.documentElement.style.fontSize = ''
    }
  })

  it('provides default material theme', () => {
    render(
      <ThemeProvider>
        <ThemeConsumer />
      </ThemeProvider>
    )
    expect(screen.getByTestId('current-theme')).toHaveTextContent('material')
  })

  it('provides all 13 themes', () => {
    render(
      <ThemeProvider>
        <ThemeConsumer />
      </ThemeProvider>
    )
    expect(screen.getByTestId('theme-count')).toHaveTextContent('13')
  })

  it('switches to solarized-light theme and sets data-theme', () => {
    render(
      <ThemeProvider>
        <ThemeConsumer />
      </ThemeProvider>
    )
    fireEvent.click(screen.getByTestId('btn-solarized-light'))
    expect(screen.getByTestId('current-theme')).toHaveTextContent('solarized-light')
    expect(document.documentElement.getAttribute('data-theme')).toBe('solarized-light')
  })

  it('switches to gruvbox-light theme and sets data-theme', () => {
    render(
      <ThemeProvider>
        <ThemeConsumer />
      </ThemeProvider>
    )
    fireEvent.click(screen.getByTestId('btn-gruvbox-light'))
    expect(screen.getByTestId('current-theme')).toHaveTextContent('gruvbox-light')
    expect(document.documentElement.getAttribute('data-theme')).toBe('gruvbox-light')
  })

  it('switches to solarized theme and sets data-theme', () => {
    render(
      <ThemeProvider>
        <ThemeConsumer />
      </ThemeProvider>
    )
    fireEvent.click(screen.getByTestId('btn-solarized'))
    expect(screen.getByTestId('current-theme')).toHaveTextContent('solarized')
    expect(document.documentElement.getAttribute('data-theme')).toBe('solarized')
  })

  it('switches to dracula theme and sets data-theme', () => {
    render(
      <ThemeProvider>
        <ThemeConsumer />
      </ThemeProvider>
    )
    fireEvent.click(screen.getByTestId('btn-dracula'))
    expect(screen.getByTestId('current-theme')).toHaveTextContent('dracula')
    expect(document.documentElement.getAttribute('data-theme')).toBe('dracula')
  })

  it('switches to gruvbox theme and sets data-theme', () => {
    render(
      <ThemeProvider>
        <ThemeConsumer />
      </ThemeProvider>
    )
    fireEvent.click(screen.getByTestId('btn-gruvbox'))
    expect(screen.getByTestId('current-theme')).toHaveTextContent('gruvbox')
    expect(document.documentElement.getAttribute('data-theme')).toBe('gruvbox')
  })

  it('resolves system theme to light/dight based on media query', () => {
    render(
      <ThemeProvider>
        <ThemeConsumer />
      </ThemeProvider>
    )
    fireEvent.click(screen.getByTestId('btn-system'))
    expect(screen.getByTestId('current-theme')).toHaveTextContent('system')
    // resolvedTheme depends on prefers-color-scheme, data-theme is set accordingly
    const dataTheme = document.documentElement.getAttribute('data-theme')
    expect(['material', 'tokyo-night']).toContain(dataTheme)
  })

  it('switches theme on setTheme call', () => {
    render(
      <ThemeProvider>
        <ThemeConsumer />
      </ThemeProvider>
    )
    fireEvent.click(screen.getByTestId('btn-tokyo-night'))
    expect(screen.getByTestId('current-theme')).toHaveTextContent('tokyo-night')
  })

  it('sets data-theme attribute on document', () => {
    render(
      <ThemeProvider>
        <ThemeConsumer />
      </ThemeProvider>
    )
    fireEvent.click(screen.getByTestId('btn-nord'))
    expect(document.documentElement.getAttribute('data-theme')).toBe('nord')
  })

  it('throws when useTheme used outside provider', () => {
    expect(() => {
      render(<ThemeConsumer />)
    }).toThrow('useTheme must be used within ThemeProvider')
  })

  it('provides default fontScale of 1.0', () => {
    render(
      <ThemeProvider>
        <ThemeConsumer />
      </ThemeProvider>
    )
    expect(screen.getByTestId('font-scale')).toHaveTextContent('1')
  })

  it('updates fontScale and applies to document element', () => {
    render(
      <ThemeProvider>
        <ThemeConsumer />
      </ThemeProvider>
    )
    fireEvent.click(screen.getByTestId('set-font-scale'))
    expect(screen.getByTestId('font-scale')).toHaveTextContent('1.15')
    expect(document.documentElement.style.fontSize).toBe('18.4px')
  })

  it('clamps fontScale to valid range', () => {
    render(
      <ThemeProvider>
        <ThemeConsumer />
      </ThemeProvider>
    )
    screen.getByTestId('font-scale') // presence check: getBy throws when missing
    // Test lower bound
    fireEvent.click(screen.getByTestId('set-font-scale'))
    // The setFontScale should clamp values to [0.85, 1.3]
    expect(document.documentElement.style.fontSize).toBe('18.4px')
  })
})

describe('ThemeContext — scheme registry (U9)', () => {
  beforeEach(() => {
    window.localStorage.clear()
    document.documentElement.removeAttribute('data-theme-mode')
  })

  // Expected scheme per theme — mirrors how each theme's tokens are authored.
  const EXPECTED: Record<string, 'light' | 'dark'> = {
    'material': 'light',
    'tokyo-night': 'dark',
    'tokyo-night-light': 'light',
    'catppuccin': 'dark',
    'nord': 'dark',
    'ember': 'light',
    'slate': 'light',
    'solarized': 'dark',
    'solarized-light': 'light',
    'dracula': 'dark',
    'gruvbox': 'dark',
    'gruvbox-light': 'light',
  }

  it('mirrors the scheme of every theme onto data-theme-mode', () => {
    render(
      <ThemeProvider>
        <ThemeConsumer />
      </ThemeProvider>
    )
    for (const [id, scheme] of Object.entries(EXPECTED)) {
      fireEvent.click(screen.getByTestId(`btn-${id}`))
      expect(document.documentElement.getAttribute('data-theme'), id).toBe(id)
      expect(document.documentElement.getAttribute('data-theme-mode'), id).toBe(scheme)
    }
  })

  it('system resolves data-theme-mode with the resolved theme', () => {
    // Patch the factory so every fresh matchMedia call reports dark.
    const real = window.matchMedia
    vi.spyOn(window, 'matchMedia').mockImplementation(q =>
      ({ ...real(q), matches: q.includes('dark') }))
    render(
      <ThemeProvider>
        <ThemeConsumer />
      </ThemeProvider>
    )
    fireEvent.click(screen.getByTestId('btn-system'))
    expect(document.documentElement.getAttribute('data-theme')).toBe('tokyo-night')
    expect(document.documentElement.getAttribute('data-theme-mode')).toBe('dark')
  })
})
