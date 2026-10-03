import { createContext, useContext, useState, useEffect, useCallback, type ReactNode } from 'react'
import * as api from '@/lib/tauri-api'
import { THEME_REGISTRY } from '@/theme/generated/registry'

export type ThemeName = 'material' | 'tokyo-night' | 'tokyo-night-light' | 'catppuccin' | 'nord' | 'ember' | 'slate' | 'solarized' | 'solarized-light' | 'dracula' | 'gruvbox' | 'gruvbox-light' | 'system'

type ResolvedTheme = Exclude<ThemeName, 'system'>

/** The light/dark scheme each resolved theme renders as — mirrors THEME_SCHEMES. */
export function themeModeOf(resolved: ResolvedTheme): 'light' | 'dark' {
  return THEME_SCHEMES[resolved]
}

interface ThemeContextValue {
  theme: ThemeName
  setTheme: (theme: ThemeName) => void
  resolvedTheme: ResolvedTheme
  themes: { id: ThemeName; labelKey: string }[]
  fontScale: number
  setFontScale: (scale: number) => void
}

const ThemeContext = createContext<ThemeContextValue | null>(null)

export function useTheme() {
  const ctx = useContext(ThemeContext)
  if (!ctx) throw new Error('useTheme must be used within ThemeProvider')
  return ctx
}

// U9/U-themes: the scheme each theme renders as comes from the GENERATED
// registry (scripts/theme-source.json → scripts/generate-themes.mjs) — the
// same single source that emits the token blocks in
// src/theme/generated/themes.css. ThemeProvider mirrors it onto
// <html data-theme-mode>, and index.css's `dark:` variant keys off that
// attribute. The registry must match the token blocks: e2e/themes.spec.ts
// asserts each theme's surface luminance against its registered scheme, so
// a mismatch (e.g. a light token block registered as 'dark', which made the
// `dark:` variants misfire on ember/slate) fails CI instead of shipping.
const THEME_SCHEMES = Object.fromEntries(
  THEME_REGISTRY.map(entry => [entry.id, entry.mode]),
) as Record<ResolvedTheme, 'light' | 'dark'>

// Compile-time drift tripwire: the generated registry must cover exactly the
// ResolvedTheme union — a theme added to the type (or to theme-source.json)
// without the other fails here.
type RegistryTheme = (typeof THEME_REGISTRY)[number]['id']
type RegistryMissing = [Exclude<ResolvedTheme, RegistryTheme>] extends [never] ? true : false
type RegistryExtra = [Exclude<RegistryTheme, ResolvedTheme>] extends [never] ? true : false
const REGISTRY_ALIGNED: [RegistryMissing, RegistryExtra] = [true, true]
void REGISTRY_ALIGNED

// G7 i18n (P1-8): theme display names are intl message ids
// (`settings.theme.name.*`) resolved by the consumer (ThemeSettings) — the
// provider renders outside <IntlProvider>, so it carries keys, not strings.
// Built from the literal ids so the compiler flags any drift with ThemeName.
const THEMES: { id: ThemeName; labelKey: string }[] = (
  [
    'system',
    'material',
    'tokyo-night',
    'tokyo-night-light',
    'catppuccin',
    'nord',
    'ember',
    'slate',
    'solarized',
    'solarized-light',
    'dracula',
    'gruvbox',
    'gruvbox-light',
  ] as const
).map((id) => ({
  id,
  // kebab-case theme id → camelCase message-id suffix ('tokyo-night' → tokyoNight).
  labelKey: `settings.theme.name.${id.replace(/-([a-z])/g, (_, c: string) => c.toUpperCase())}`,
}))

function getSystemTheme(): ResolvedTheme {
  if (typeof window === 'undefined') return 'material'
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'tokyo-night' : 'material'
}

export function ThemeProvider({ children }: { children: ReactNode }) {
  // Dark-first default (UI audit §2.5: all four competitors ship dark as the
  // default or flagship look). Users who picked a theme keep theirs via
  // localStorage; 'system' still resolves per OS preference.
  const [theme, setThemeState] = useState<ThemeName>(() => {
    if (typeof window !== 'undefined') {
      return (localStorage.getItem('shannon-theme') as ThemeName) || 'tokyo-night'
    }
    return 'tokyo-night'
  })

  const [fontScale, setFontScaleState] = useState<number>(() => {
    if (typeof window !== 'undefined') {
      const stored = localStorage.getItem('shannon.fontScale')
      return stored ? parseFloat(stored) : 1.0
    }
    return 1.0
  })

  // F-theme-system: the OS scheme lives in STATE, not in a render-time read.
  // resolvedTheme used to call getSystemTheme() during render while the
  // prefers-color-scheme listener "refreshed" it via setThemeState('system')
  // — the SAME value, which hits React's eager bail-out: no re-render, so a
  // live OS light↔dark switch never recomputed resolvedTheme and
  // data-theme/data-theme-mode went stale. Writing the REAL new value below
  // is a genuine state change (and if the new scheme truly equals the stored
  // one, skipping the re-render is correct).
  const [systemTheme, setSystemTheme] = useState<ResolvedTheme>(getSystemTheme)

  const resolvedTheme: ResolvedTheme = theme === 'system' ? systemTheme : theme

  useEffect(() => {
    document.documentElement.setAttribute('data-theme', resolvedTheme)
    document.documentElement.setAttribute('data-theme-mode', THEME_SCHEMES[resolvedTheme])
    if (theme !== 'system') {
      localStorage.setItem('shannon-theme', theme)
    }
  }, [theme, resolvedTheme])

  useEffect(() => {
    const baseFontSize = 16 * fontScale
    document.documentElement.style.fontSize = `${baseFontSize}px`
    localStorage.setItem('shannon.fontScale', fontScale.toString())
  }, [fontScale])

  // Attached for every mode (not just theme='system'): an OS flip while the
  // user sits on an explicit theme must still update systemTheme, or a later
  // switch back to 'system' would resolve from a stale scheme. Non-color
  // system settings (fontScale, contrast) have their own seams and are
  // untouched by this listener.
  useEffect(() => {
    const mq = window.matchMedia('(prefers-color-scheme: dark)')
    const handler = () => setSystemTheme(getSystemTheme())
    mq.addEventListener('change', handler)
    return () => mq.removeEventListener('change', handler)
  }, [])

  const setTheme = useCallback((newTheme: ThemeName) => {
    setThemeState(newTheme)
    localStorage.setItem('shannon-theme', newTheme)
    api.configure({ key: 'theme', value: newTheme }).catch(e => console.warn('Failed to save theme:', e))
  }, [])

  const setFontScale = useCallback((scale: number) => {
    const clamped = Math.max(0.85, Math.min(1.3, scale))
    setFontScaleState(clamped)
  }, [])

  return (
    <ThemeContext.Provider value={{ theme, setTheme, resolvedTheme, themes: THEMES, fontScale, setFontScale }}>
      {children}
    </ThemeContext.Provider>
  )
}
