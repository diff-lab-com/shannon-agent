import { createContext, useContext, useState, useEffect, useCallback, type ReactNode } from 'react'
import * as api from '@/lib/tauri-api'
import { THEME_REGISTRY } from '@/theme/generated/registry'

/**
 * H1 theme convergence (2026-10-17 user ruling): Aurora Ink is the ONLY
 * user-visible theme. The 12 sibling themes in theme-source.json stay in the
 * repo as DORMANT INFRASTRUCTURE (generated token blocks + registry entries,
 * UI-unreachable — kept so the pipeline and a future re-import stay
 * rollback-cheap), but nothing in the runtime can select or render them:
 * ThemeName collapses to a single id, `system` is gone (aurora-ink is dark
 * by definition, there is no OS-follow branch), and any stored value a
 * previous build persisted (material, tokyo-night, system, …) normalizes to
 * aurora-ink on boot.
 */
export type ThemeName = 'aurora-ink'

/**
 * A resolvable `<html data-theme>` id. Still the FULL generated-registry
 * union on purpose: the dormant mapping layers (cmTheme / xtermTheme read
 * this attribute and keep per-theme tables for the hibernating themes) and
 * their unit tests stay type-honest. The provider below only ever WRITES
 * `ThemeName` — no runtime path can produce another id.
 */
type ResolvedTheme = (typeof THEME_REGISTRY)[number]['id']

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

// U9/H1: the scheme each registry id renders as comes from the GENERATED
// registry (scripts/theme-source.json → scripts/generate-themes.mjs) — the
// same single source that emits the token blocks in
// src/theme/generated/themes.css. ThemeProvider mirrors it onto
// <html data-theme-mode>, and index.css's `dark:` variant keys off that
// attribute. The dormant themes' entries are kept only so this map (and the
// cm/xterm floors keyed by themeModeOf) stay total over the registry; the
// provider writes aurora-ink exclusively.
const THEME_SCHEMES = Object.fromEntries(
  THEME_REGISTRY.map(entry => [entry.id, entry.mode]),
) as Record<ResolvedTheme, 'light' | 'dark'>

// Compile-time drift tripwire (narrowed by the H1 convergence): the single
// UI-reachable theme must remain a generated-registry id AND stay registered
// 'dark' — removing aurora-ink from theme-source.json, renaming it, or
// re-authoring it as a light scheme fails here instead of shipping a provider
// whose token block no longer exists.
const AURORA_SCHEME: (typeof THEME_SCHEMES)['aurora-ink'] = 'dark'
void AURORA_SCHEME

// G7 i18n (P1-8): theme display names are intl message ids
// (`settings.theme.name.*`) resolved by the consumer (ThemeSettings) — the
// provider renders outside <IntlProvider>, so it carries keys, not strings.
// Built from the literal ids so the compiler flags any drift with ThemeName.
// H1: exactly one entry — the dormant themes keep their locale keys deleted.
const THEMES: { id: ThemeName; labelKey: string }[] = (
  [
    'aurora-ink',
  ] as const
).map((id) => ({
  id,
  // kebab-case theme id → camelCase message-id suffix ('aurora-ink' → auroraInk).
  labelKey: `settings.theme.name.${id.replace(/-([a-z])/g, (_, c: string) => c.toUpperCase())}`,
}))

export function ThemeProvider({ children }: { children: ReactNode }) {
  // H1 theme convergence: default AND only option is 'aurora-ink' — the
  // 2026-10-15 "aurora ink" design system (ui-redesign-2026-10, review §5
  // verdict A), made the sole user-visible theme by the 2026-10-17 ruling.
  //
  // Migration (generalized from the 2026-10-08 one-time material migration):
  // ANY non-aurora-ink stored value — 'material'/'tokyo-night' leftovers of
  // the pre-H1 picker, or 'system' from the retired OS-follow mode — reads
  // as aurora-ink, and the effect below writes aurora-ink back to
  // localStorage, so the stored value is normalized on the first boot after
  // the upgrade. No flag is needed anymore: aurora-ink is the only writable
  // value, so re-normalizing on every boot is idempotent and an explicit
  // "pick" cannot exist to protect.
  const [theme, setThemeState] = useState<ThemeName>(() => {
    if (typeof window !== 'undefined') {
      const stored = localStorage.getItem('shannon-theme') as ThemeName | null
      return stored === 'aurora-ink' ? stored : 'aurora-ink'
    }
    return 'aurora-ink'
  })

  const [fontScale, setFontScaleState] = useState<number>(() => {
    if (typeof window !== 'undefined') {
      const stored = localStorage.getItem('shannon.fontScale')
      return stored ? parseFloat(stored) : 1.0
    }
    return 1.0
  })

  // resolvedTheme is the stored theme itself: with 'system' retired there is
  // no OS-scheme branch left to recompute (the old F-theme-system
  // matchMedia listener existed solely to feed it and is gone with it).
  const resolvedTheme: ResolvedTheme = theme

  useEffect(() => {
    document.documentElement.setAttribute('data-theme', theme)
    document.documentElement.setAttribute('data-theme-mode', THEME_SCHEMES[theme])
    // Write-back is also the migration commit: a pre-H1 stored value
    // (tokyo-night, system, …) lands on disk as aurora-ink.
    localStorage.setItem('shannon-theme', theme)
  }, [theme])

  useEffect(() => {
    const baseFontSize = 16 * fontScale
    document.documentElement.style.fontSize = `${baseFontSize}px`
    localStorage.setItem('shannon.fontScale', fontScale.toString())
  }, [fontScale])

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
