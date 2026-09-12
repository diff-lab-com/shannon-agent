import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from 'react'
import { IntlProvider, useIntl, type PrimitiveType } from 'react-intl'

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

export type Locale = 'en' | 'zh-CN' | 'es' | 'fr' | 'de' | 'ja' | 'ko' | 'pt-BR' | 'ru' | 'zh-TW'

const LOCALE_STORAGE_KEY = 'shannon.locale'

// Locale files are loaded eagerly to keep the typed MESSAGES map simple.
// New locales copy en.json as a fallback (with a `_meta.status: fallback-en`
// marker) and get translated over time. Missing keys resolve to English at
// runtime via the `?? MESSAGES.en[id]` fallback in `t()` below.
import es from './locales/es.json'
import fr from './locales/fr.json'
import de from './locales/de.json'
import ja from './locales/ja.json'
import ko from './locales/ko.json'
import ptBR from './locales/pt-BR.json'
import ru from './locales/ru.json'
import zhTW from './locales/zh-TW.json'

const MESSAGES: Record<Locale, Record<string, string>> = {
  en: en as Record<string, string>,
  'zh-CN': zhCN as Record<string, string>,
  es: es as Record<string, string>,
  fr: fr as Record<string, string>,
  de: de as Record<string, string>,
  ja: ja as Record<string, string>,
  ko: ko as Record<string, string>,
  'pt-BR': ptBR as Record<string, string>,
  ru: ru as Record<string, string>,
  'zh-TW': zhTW as Record<string, string>,
}

/** Detect a sensible default locale. Browser language → supported; else `en`. */
function detectDefault(): Locale {
  if (typeof window === 'undefined') return 'en'
  const stored = window.localStorage.getItem(LOCALE_STORAGE_KEY) as Locale | null
  if (stored && stored in MESSAGES) return stored
  const nav = window.navigator?.language?.toLowerCase() ?? ''
  if (nav.startsWith('zh-tw') || nav.startsWith('zh-hk')) return 'zh-TW'
  if (nav.startsWith('zh')) return 'zh-CN'
  if (nav.startsWith('ja')) return 'ja'
  if (nav.startsWith('ko')) return 'ko'
  if (nav.startsWith('es')) return 'es'
  if (nav.startsWith('fr')) return 'fr'
  if (nav.startsWith('de')) return 'de'
  if (nav.startsWith('pt')) return 'pt-BR'
  if (nav.startsWith('ru')) return 'ru'
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

/**
 * Stable `t(id)` — `intl.formatMessage` bound per locale switch. A raw
 * `const t = (id) => intl.formatMessage({ id })` creates a fresh function
 * every render, which trips react-hooks/exhaustive-deps as soon as `t`
 * (or a callback capturing it) lands in a hook dependency array. `intl`
 * itself is referentially stable while locale+messages stay unchanged,
 * so `[intl]` keeps dependent useCallback/useMemo/useEffect from churning.
 *
 * @example
 * const t = useT()
 * t('settings.title')
 */
export function useT(): (id: string, values?: Record<string, PrimitiveType>) => string {
  const intl = useIntl()
  return useCallback(
    (id: string, values?: Record<string, PrimitiveType>) => intl.formatMessage({ id }, values),
    [intl],
  )
}

/**
 * Provider-independent message lookup for non-component contexts — e.g. a
 * context provider rendered beside (not inside) `<IntlProvider>` that still
 * needs a translated user-facing string. Reads the persisted locale the same
 * way `I18nProvider` does; falls back to `en`, then to the raw id.
 */
export function messageFor(id: string, values?: Record<string, PrimitiveType>): string {
  let locale: Locale = 'en'
  if (typeof window !== 'undefined') {
    const stored = window.localStorage.getItem(LOCALE_STORAGE_KEY)
    if (stored === 'en' || stored === 'zh-CN') locale = stored
    else if ((window.navigator?.language?.toLowerCase() ?? '').startsWith('zh')) locale = 'zh-CN'
  }
  const tpl = MESSAGES[locale][id] ?? MESSAGES.en[id] ?? id
  if (!values) return tpl
  return tpl.replace(/\{(\w+)\}/g, (_, k: string) =>
    values[k] !== undefined ? String(values[k]) : `{${k}}`,
  )
}

/** Convenience: list of supported locales for switcher UIs. */
export const SUPPORTED_LOCALES: ReadonlyArray<{ id: Locale; labelKey: string }> = [
  { id: 'en', labelKey: 'settings.language.en' },
  { id: 'zh-CN', labelKey: 'settings.language.zhCN' },
  { id: 'zh-TW', labelKey: 'settings.language.zhTW' },
  { id: 'ja', labelKey: 'settings.language.ja' },
  { id: 'ko', labelKey: 'settings.language.ko' },
  { id: 'es', labelKey: 'settings.language.es' },
  { id: 'fr', labelKey: 'settings.language.fr' },
  { id: 'de', labelKey: 'settings.language.de' },
  { id: 'pt-BR', labelKey: 'settings.language.ptBR' },
  { id: 'ru', labelKey: 'settings.language.ru' },
]
