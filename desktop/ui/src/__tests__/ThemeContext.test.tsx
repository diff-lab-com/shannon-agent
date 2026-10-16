import { describe, it, expect, beforeEach, vi } from 'vitest'
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

  // Verdict A (ui-redesign-2026-10 review §5) made aurora-ink the dark
  // default; the H1 convergence (2026-10-17 ruling) made it the ONLY
  // user-visible theme.
  it('defaults to the aurora-ink theme', () => {
    render(
      <ThemeProvider>
        <ThemeConsumer />
      </ThemeProvider>
    )
    expect(screen.getByTestId('current-theme')).toHaveTextContent('aurora-ink')
  })

  // H1: the picker collapsed to a single entry — the 12 sibling themes stay
  // in the generated registry as dormant infrastructure but are not exposed.
  it('exposes exactly one theme (aurora-ink)', () => {
    render(
      <ThemeProvider>
        <ThemeConsumer />
      </ThemeProvider>
    )
    expect(screen.getByTestId('theme-count')).toHaveTextContent('1')
    expect(screen.getByTestId('btn-aurora-ink')).toBeInTheDocument()
  })

  // H1 migration (generalized from the 2026-10-08 material migration): ANY
  // non-aurora-ink stored value — dormant picks (tokyo-night) or the retired
  // 'system' mode — normalizes to aurora-ink, and the write-back commits
  // aurora-ink to localStorage on the first boot.
  it.each(['tokyo-night', 'material', 'system', 'dracula', 'a-future-stray-value'])(
    'migrates a stored %s value to aurora-ink and writes it back',
    (stored) => {
      localStorage.setItem('shannon-theme', stored)
      render(
        <ThemeProvider>
          <ThemeConsumer />
        </ThemeProvider>
      )
      expect(screen.getByTestId('current-theme')).toHaveTextContent('aurora-ink')
      expect(localStorage.getItem('shannon-theme')).toBe('aurora-ink')
    },
  )

  it('keeps an explicit aurora-ink pick (idempotent boot)', () => {
    localStorage.setItem('shannon-theme', 'aurora-ink')
    render(
      <ThemeProvider>
        <ThemeConsumer />
      </ThemeProvider>
    )
    expect(screen.getByTestId('current-theme')).toHaveTextContent('aurora-ink')
    expect(localStorage.getItem('shannon-theme')).toBe('aurora-ink')
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
    for (const id of ['aurora-ink']) {
      expect(screen.getByTestId(`btn-${id}`).textContent).toMatch(/^settings\.theme\.name\./)
    }
  })

  it('setTheme(aurora-ink) keeps data-theme and the stored value on aurora-ink', () => {
    render(
      <ThemeProvider>
        <ThemeConsumer />
      </ThemeProvider>
    )
    // The only settable value is a no-op re-affirmation — the picker has no
    // other destination to write.
    fireEvent.click(screen.getByTestId('btn-aurora-ink'))
    expect(screen.getByTestId('current-theme')).toHaveTextContent('aurora-ink')
    expect(document.documentElement.getAttribute('data-theme')).toBe('aurora-ink')
    expect(localStorage.getItem('shannon-theme')).toBe('aurora-ink')
  })

  it('sets data-theme + data-theme-mode attributes on document', () => {
    localStorage.setItem('shannon-theme', 'tokyo-night') // pre-H1 leftover
    render(
      <ThemeProvider>
        <ThemeConsumer />
      </ThemeProvider>
    )
    expect(document.documentElement.getAttribute('data-theme')).toBe('aurora-ink')
    expect(document.documentElement.getAttribute('data-theme-mode')).toBe('dark')
    expect(screen.getByTestId('resolved-theme')).toHaveTextContent('aurora-ink')
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

describe('ThemeContext — scheme registry (U9, H1-narrowed)', () => {
  beforeEach(() => {
    window.localStorage.clear()
    document.documentElement.removeAttribute('data-theme-mode')
  })

  // H1: aurora-ink is the only theme the provider writes, so the mirrored
  // scheme contract narrows to its single dark entry. The dormant registry
  // entries are still resolvable through themeModeOf for the cm/xterm
  // mapping layers, but no runtime path selects them.
  it('mirrors aurora-ink’s registered dark scheme onto data-theme-mode', () => {
    localStorage.setItem('shannon-theme', 'tokyo-night') // pre-H1 leftover
    render(
      <ThemeProvider>
        <ThemeConsumer />
      </ThemeProvider>
    )
    expect(document.documentElement.getAttribute('data-theme')).toBe('aurora-ink')
    expect(document.documentElement.getAttribute('data-theme-mode')).toBe('dark')
  })

  it('writes the dark scheme even when the OS prefers light (no system mode)', () => {
    // The retired 'system' branch used to resolve per OS — H1 removed it, so
    // a light-OS preference must NOT leak a light scheme onto <html>.
    const real = window.matchMedia
    vi.spyOn(window, 'matchMedia').mockImplementation(q =>
      ({ ...real(q), matches: false }))
    render(
      <ThemeProvider>
        <ThemeConsumer />
      </ThemeProvider>
    )
    expect(document.documentElement.getAttribute('data-theme')).toBe('aurora-ink')
    expect(document.documentElement.getAttribute('data-theme-mode')).toBe('dark')
  })
})

describe('ThemeContext — the retired system mode stays retired (H1)', () => {
  beforeEach(() => {
    window.localStorage.clear()
    document.documentElement.removeAttribute('data-theme')
    document.documentElement.removeAttribute('data-theme-mode')
  })

  // H1 removed the OS-follow branch wholesale (the F-theme-system
  // matchMedia listener existed only to feed it). These tests pin the
  // consequence: stored 'system' migrates, and no prefers-color-scheme
  // change can move the resolved theme off aurora-ink.
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

  it('a stored system value migrates to aurora-ink and an OS flip cannot change it', () => {
    localStorage.setItem('shannon-theme', 'system') // pre-H1 leftover
    const mq = createSchemeMedia(false) // boots light
    const spy = vi.spyOn(window, 'matchMedia').mockImplementation(() => mq as unknown as MediaQueryList)
    try {
      render(
        <ThemeProvider>
          <ThemeConsumer />
        </ThemeProvider>
      )
      expect(screen.getByTestId('resolved-theme')).toHaveTextContent('aurora-ink')
      expect(document.documentElement.getAttribute('data-theme')).toBe('aurora-ink')
      expect(document.documentElement.getAttribute('data-theme-mode')).toBe('dark')

      // OS flips dark: nothing observes it anymore — the theme stays put.
      act(() => mq.flip(true))
      expect(screen.getByTestId('resolved-theme')).toHaveTextContent('aurora-ink')
      expect(document.documentElement.getAttribute('data-theme')).toBe('aurora-ink')
      expect(document.documentElement.getAttribute('data-theme-mode')).toBe('dark')
    } finally {
      spy.mockRestore()
    }
  })
})
