import { createContext, useContext, useState, useEffect, useCallback, type ReactNode } from 'react'
import * as api from '@/lib/tauri-api'

export type ThemeName = 'material' | 'tokyo-night' | 'tokyo-night-light' | 'catppuccin' | 'nord' | 'ember' | 'slate' | 'solarized' | 'dracula' | 'gruvbox' | 'system'

type ResolvedTheme = Exclude<ThemeName, 'system'>

interface ThemeContextValue {
  theme: ThemeName
  setTheme: (theme: ThemeName) => void
  resolvedTheme: ResolvedTheme
  themes: { id: ThemeName; label: string }[]
}

const ThemeContext = createContext<ThemeContextValue | null>(null)

export function useTheme() {
  const ctx = useContext(ThemeContext)
  if (!ctx) throw new Error('useTheme must be used within ThemeProvider')
  return ctx
}

const THEMES: { id: ThemeName; label: string }[] = [
  { id: 'system', label: 'System' },
  { id: 'material', label: 'Material' },
  { id: 'tokyo-night', label: 'Tokyo Night' },
  { id: 'tokyo-night-light', label: 'Tokyo Night Light' },
  { id: 'catppuccin', label: 'Catppuccin' },
  { id: 'nord', label: 'Nord' },
  { id: 'ember', label: 'Ember' },
  { id: 'slate', label: 'Slate' },
  { id: 'solarized', label: 'Solarized Dark' },
  { id: 'dracula', label: 'Dracula' },
  { id: 'gruvbox', label: 'Gruvbox' },
]

function getSystemTheme(): ResolvedTheme {
  if (typeof window === 'undefined') return 'material'
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'tokyo-night' : 'material'
}

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [theme, setThemeState] = useState<ThemeName>(() => {
    if (typeof window !== 'undefined') {
      return (localStorage.getItem('shannon-theme') as ThemeName) || 'material'
    }
    return 'material'
  })

  const resolvedTheme: ResolvedTheme = theme === 'system' ? getSystemTheme() : theme

  useEffect(() => {
    document.documentElement.setAttribute('data-theme', resolvedTheme)
    if (theme !== 'system') {
      localStorage.setItem('shannon-theme', theme)
    }
  }, [theme, resolvedTheme])

  useEffect(() => {
    if (theme !== 'system') return
    const mq = window.matchMedia('(prefers-color-scheme: dark)')
    const handler = () => setThemeState('system') // triggers re-render with new resolvedTheme
    mq.addEventListener('change', handler)
    return () => mq.removeEventListener('change', handler)
  }, [theme])

  const setTheme = useCallback((newTheme: ThemeName) => {
    setThemeState(newTheme)
    localStorage.setItem('shannon-theme', newTheme)
    api.configure({ key: 'theme', value: newTheme }).catch(e => console.warn('Failed to save theme:', e))
  }, [])

  return (
    <ThemeContext.Provider value={{ theme, setTheme, resolvedTheme, themes: THEMES }}>
      {children}
    </ThemeContext.Provider>
  )
}
