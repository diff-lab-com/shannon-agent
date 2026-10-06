import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, screen, fireEvent, act } from '@testing-library/react'
import { ThemeProvider, useTheme } from '@/context/ThemeContext'

function ThemeConsumer() {
  const { theme, setTheme, resolvedTheme, themes, fontScale, setFontScale } = useTheme()
  return (
    <div>
      <span data-testid="current-theme">{theme}</span>
      <span data-testid="resolved-theme">{resolvedTheme}</span>
      <span data-testid="theme-count">{themes.length}</span>
      <span data-testid="font-scale">{fontScale.toString()}</span>
      <button data-testid="set-font-scale" onClick={() => setFontScale(1.15)}>
        Set Font Scale
      </button>
      {themes.map(t => (
        <button key={t.id} data-testid={`btn-${t.id}`} onClick={() => setTheme(t.id)}>
          {t.labelKey}
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

  it('defaults to the dark-first tokyo-night theme', () => {
    render(
      <ThemeProvider>
        <ThemeConsumer />
      </ThemeProvider>
    )
    expect(screen.getByTestId('current-theme')).toHaveTextContent('tokyo-night')
  })

  it('provides all 13 themes', () => {
    render(
      <ThemeProvider>
        <ThemeConsumer />
      </ThemeProvider>
    )
    expect(screen.getByTestId('theme-count')).toHaveTextContent('13')
  })

  // G7 i18n (P1-8): the provider carries message ids, not strings — every
  // id must sit in the `settings.theme.name.*` namespace the locale files
  // define, or ThemeSettings renders a raw key.
  it('maps every theme to a settings.theme.name.* message id', () => {
    render(
      <ThemeProvider>
        <ThemeConsumer />
      </ThemeProvider>
    )
    for (const id of [
      'system', 'material', 'tokyo-night', 'tokyo-night-light', 'catppuccin',
      'nord', 'ember', 'slate', 'solarized', 'solarized-light', 'dracula',
      'gruvbox', 'gruvbox-light',
    ]) {
      expect(screen.getByTestId(`btn-${id}`).textContent).toMatch(/^settings\.theme\.name\./)
    }
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

describe('ThemeContext — live OS scheme switch in system mode (F-theme-system)', () => {
  beforeEach(() => {
    window.localStorage.clear()
    document.documentElement.removeAttribute('data-theme')
    document.documentElement.removeAttribute('data-theme-mode')
  })

  let matchMediaSpy: ReturnType<typeof vi.spyOn> | undefined

  afterEach(() => {
    // Targeted restore — vi.restoreAllMocks() would also wipe the setup
    // file's global mocks (e.g. tauri-api configure → mockResolvedValue).
    // Tests that never installed the spy must not crash the teardown.
    matchMediaSpy?.mockRestore()
    matchMediaSpy = undefined
  })

  // A controllable matchMedia: the provider registers its change listener on
  // the object this factory returns, and `flip` mutates `matches` + fires the
  // listeners exactly like a real prefers-color-scheme change does.
  function createSchemeMedia(startsDark: boolean) {
    type SchemeListener = (e: MediaQueryListEvent) => void
    const listeners = new Set<SchemeListener>()
    const mql = {
      matches: startsDark,
      media: '(prefers-color-scheme: dark)',
      onchange: null,
      addEventListener: (type: string, cb: SchemeListener) => {
        if (type === 'change') listeners.add(cb)
      },
      removeEventListener: (type: string, cb: SchemeListener) => {
        if (type === 'change') listeners.delete(cb)
      },
      addListener: vi.fn(),
      removeListener: vi.fn(),
      dispatchEvent: vi.fn(),
      flip(dark: boolean) {
        mql.matches = dark
        const event = { matches: dark, media: mql.media } as MediaQueryListEvent
        for (const cb of [...listeners]) cb(event)
      },
    }
    return mql
  }

  function mountWithMedia(mql: ReturnType<typeof createSchemeMedia>) {
    matchMediaSpy = vi.spyOn(window, 'matchMedia').mockImplementation(() => mql as unknown as MediaQueryList)
    return render(
      <ThemeProvider>
        <ThemeConsumer />
      </ThemeProvider>
    )
  }

  it('recomputes resolvedTheme + data-theme(-mode) when the OS flips light↔dark', () => {
    const mq = createSchemeMedia(false) // boot light → material
    mountWithMedia(mq)

    fireEvent.click(screen.getByTestId('btn-system'))
    expect(screen.getByTestId('resolved-theme')).toHaveTextContent('material')
    expect(document.documentElement.getAttribute('data-theme')).toBe('material')
    expect(document.documentElement.getAttribute('data-theme-mode')).toBe('light')

    // OS flips to dark — the listener must write the REAL new scheme (a
    // same-value setState bail-out used to swallow this: F-theme-system).
    act(() => mq.flip(true))
    expect(screen.getByTestId('resolved-theme')).toHaveTextContent('tokyo-night')
    expect(document.documentElement.getAttribute('data-theme')).toBe('tokyo-night')
    expect(document.documentElement.getAttribute('data-theme-mode')).toBe('dark')

    // And back.
    act(() => mq.flip(false))
    expect(screen.getByTestId('resolved-theme')).toHaveTextContent('material')
    expect(document.documentElement.getAttribute('data-theme')).toBe('material')
    expect(document.documentElement.getAttribute('data-theme-mode')).toBe('light')
  })

  it('keeps tracking OS flips that happened while an explicit theme was active', () => {
    // The listener must not be scoped to theme='system': a flip while the
    // user sits on dracula must not leave a stale scheme for the later
    // switch back to 'system'.
    const mq = createSchemeMedia(false)
    mountWithMedia(mq)

    fireEvent.click(screen.getByTestId('btn-dracula'))
    act(() => mq.flip(true))
    // Explicit theme is untouched by the OS flip...
    expect(screen.getByTestId('resolved-theme')).toHaveTextContent('dracula')
    expect(document.documentElement.getAttribute('data-theme')).toBe('dracula')
    expect(document.documentElement.getAttribute('data-theme-mode')).toBe('dark')

    // ...but switching back to system resolves from the CURRENT OS scheme.
    fireEvent.click(screen.getByTestId('btn-system'))
    expect(screen.getByTestId('resolved-theme')).toHaveTextContent('tokyo-night')
    expect(document.documentElement.getAttribute('data-theme')).toBe('tokyo-night')
  })

  it('does not re-render consumers when the flip resolves to the same scheme', () => {
    // If the new scheme truthfully equals the stored one, skipping the
    // re-render is correct — the guard is that a REAL flip still lands.
    let renders = 0
    function RenderCounter() {
      renders += 1
      const { resolvedTheme } = useTheme()
      return <span data-testid="counter-theme">{resolvedTheme}</span>
    }
    const mq = createSchemeMedia(true) // boot dark → tokyo-night
    matchMediaSpy = vi.spyOn(window, 'matchMedia').mockImplementation(() => mq as unknown as MediaQueryList)
    render(
      <ThemeProvider>
        <ThemeConsumer />
        <RenderCounter />
      </ThemeProvider>
    )
    fireEvent.click(screen.getByTestId('btn-system'))
    expect(screen.getByTestId('counter-theme')).toHaveTextContent('tokyo-night')

    const afterSwitch = renders
    act(() => mq.flip(true)) // same value — no state change, no re-render
    expect(screen.getByTestId('counter-theme')).toHaveTextContent('tokyo-night')
    expect(renders).toBe(afterSwitch)

    act(() => mq.flip(false)) // real change — must land
    expect(screen.getByTestId('counter-theme')).toHaveTextContent('material')
    expect(renders).toBe(afterSwitch + 1)
  })
})
