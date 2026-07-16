import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from 'react'
import { IntlProvider } from 'react-intl'

import en from './locales/en.json'
import zhCN from './locales/zh-CN.json'

/**
 * Shannon i18n layer (#73).
 *
 * Supports English (`en`) and Simplified Chinese (`zh-CN`). The locale is
 * persisted in `localStorage` (`shannon.locale`) and falls back to the
 * browser language on first visit. Components consume messages via the
 * `useI18n()` hook below or directly through `react-intl`'s `useIntl()`.
 *
 * Migration pattern is documented in `./MIGRATION.md`. Phase 1 ships
 * infrastructure + Welcome.tsx as a reference; remaining ~120 components
 * migrate incrementally in follow-up PRs.
 */

export type Locale = 'en' | 'zh-CN'

const LOCALE_STORAGE_KEY = 'shannon.locale'

const MESSAGES: Record<Locale, Record<string, string>> = {
  en: en as Record<string, string>,
  'zh-CN': zhCN as Record<string, string>,
}

/** Detect a sensible default locale. Browser language → supported; else `en`. */
function detectDefault(): Locale {
  if (typeof window === 'undefined') return 'en'
  const stored = window.localStorage.getItem(LOCALE_STORAGE_KEY)
  if (stored === 'en' || stored === 'zh-CN') return stored
  const nav = window.navigator?.language?.toLowerCase() ?? ''
  if (nav.startsWith('zh')) return 'zh-CN'
  return 'en'
}

interface I18nContextValue {
  locale: Locale
  setLocale: (next: Locale) => void
}

const I18nContext = createContext<I18nContextValue | null>(null)

export function I18nProvider({ children }: { children: ReactNode }) {
  const [locale, setLocaleState] = useState<Locale>(detectDefault)

  // Keep `<html lang>` in sync so screen readers / browser UI match.
  useEffect(() => {
    if (typeof document !== 'undefined') {
      document.documentElement.lang = locale
    }
  }, [locale])

  const setLocale = useCallback((next: Locale) => {
    setLocaleState(next)
    if (typeof window !== 'undefined') {
      window.localStorage.setItem(LOCALE_STORAGE_KEY, next)
    }
  }, [])

  const value = useMemo<I18nContextValue>(() => ({ locale, setLocale }), [locale, setLocale])

  return (
    <IntlProvider locale={locale} defaultLocale="en" messages={MESSAGES[locale]}>
      <I18nContext.Provider value={value}>{children}</I18nContext.Provider>
    </IntlProvider>
  )
}

/**
 * Access the current locale + setter. Use this in components that need to
 * render the language switcher; for translated strings, prefer `useIntl()`
 * from react-intl directly.
 *
 * @example
 * const { locale, setLocale } = useI18n()
 * setLocale('zh-CN')
 */
export function useI18n(): I18nContextValue {
  const ctx = useContext(I18nContext)
  if (!ctx) {
    throw new Error('useI18n must be used inside <I18nProvider>')
  }
  return ctx
}

/** Convenience: list of supported locales for switcher UIs. */
export const SUPPORTED_LOCALES: ReadonlyArray<{ id: Locale; labelKey: string }> = [
  { id: 'en', labelKey: 'settings.language.en' },
  { id: 'zh-CN', labelKey: 'settings.language.zhCN' },
]
