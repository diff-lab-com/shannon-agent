import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from 'react'
import { IntlProvider, useIntl, type PrimitiveType } from 'react-intl'

import en from './locales/en.json'
import zhCN from './locales/zh-CN.json'

/**
 * Shannon i18n layer (#73).
 *
 * Supports English (`en`) and Simplified Chinese (`zh-CN`). The locale
 * preference is persisted in `localStorage` (`shannon.locale`) as either a
 * concrete locale or the literal `'system'` ("follow the OS language", also
 * the first-visit default — probed from `navigator.languages`). Components
 * consume messages via the `useI18n()` hook below or directly through
 * `react-intl`'s `useIntl()`.
 *
 * Migration pattern is documented in `./MIGRATION.md`. Phase 1 ships
 * infrastructure + Welcome.tsx as a reference; remaining ~120 components
 * migrate incrementally in follow-up PRs.
 */

export type Locale = 'en' | 'zh-CN' | 'es' | 'fr' | 'de' | 'ja' | 'ko' | 'pt-BR' | 'ru' | 'zh-TW'

/**
 * What the user picked in Settings. `'system'` means "follow the operating
 * system language"; anything else pins a concrete locale. Persisted
 * verbatim in `localStorage` (`shannon.locale`).
 */
export type LocalePref = 'system' | Locale

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

/** Map one raw navigator language tag to a supported Locale, or null. */
function matchNavigatorLanguage(tag: string): Locale | null {
  const l = (tag ?? '').toLowerCase()
  if (l.startsWith('zh-tw') || l.startsWith('zh-hk')) return 'zh-TW'
  if (l.startsWith('zh')) return 'zh-CN'
  if (l.startsWith('ja')) return 'ja'
  if (l.startsWith('ko')) return 'ko'
  if (l.startsWith('es')) return 'es'
  if (l.startsWith('fr')) return 'fr'
  if (l.startsWith('de')) return 'de'
  if (l.startsWith('pt')) return 'pt-BR'
  if (l.startsWith('ru')) return 'ru'
  return null
}

/**
 * Read the persisted preference. `'system'` and concrete locales pass
 * through; a missing or unknown value means "follow system" — the same
 * observable behavior the old detect-on-boot gave first-run users, so
 * existing stored locales keep their exact behavior.
 */
function getStoredLocalePref(): LocalePref {
  if (typeof window === 'undefined') return 'system'
  const stored = window.localStorage.getItem(LOCALE_STORAGE_KEY)
  if (stored === 'system') return 'system'
  if (stored && stored in MESSAGES) return stored as Locale
  return 'system'
}

/**
 * Pure preference → locale resolver (shared by the provider, the settings
 * switcher semantics and `messageFor`).
 *
 * - A concrete supported locale passes straight through.
 * - `'system'` (or a missing/unknown pref) probes `navigator.languages`
 *   LIVE on every call — deliberately uncached, so a window opened after
 *   the OS language changes follows it.
 * - Nothing matches → `'en'`.
 *
 * The `languages` parameter is injectable for pure unit tests; production
 * callers omit it and the global navigator is read.
 */
export function resolveLocale(
  pref: string | null | undefined,
  languages: readonly string[] | undefined = typeof navigator !== 'undefined'
    ? navigator.languages
    : undefined,
): Locale {
  if (pref && pref !== 'system' && pref in MESSAGES) return pref as Locale
  for (const tag of languages ?? []) {
    const hit = matchNavigatorLanguage(tag)
    if (hit) return hit
  }
  return 'en'
}

interface I18nContextValue {
  /** Resolved active locale — what `IntlProvider` renders with. */
  locale: Locale
  /** What the user picked: `'system'` or a pinned locale (drives the switcher). */
  localePref: LocalePref
  setLocale: (next: LocalePref) => void
}

const I18nContext = createContext<I18nContextValue | null>(null)

export function I18nProvider({ children }: { children: ReactNode }) {
  const [localePref, setPrefState] = useState<LocalePref>(getStoredLocalePref)

  // Resolved per pref change. With 'system' the probe runs on mount and on
  // every pref switch — never cached — so a window created after the OS
  // language changed picks the new language up on its first render.
  const locale = useMemo(() => resolveLocale(localePref), [localePref])

  // Keep `<html lang>` in sync so screen readers / browser UI match.
  useEffect(() => {
    if (typeof document !== 'undefined') {
      document.documentElement.lang = locale
    }
  }, [locale])

  const setLocale = useCallback((next: LocalePref) => {
    setPrefState(next)
    if (typeof window !== 'undefined') {
      // 'system' is persisted verbatim: an explicit user choice, not an
      // absent one, so it survives restarts and reads back as follow-system.
      window.localStorage.setItem(LOCALE_STORAGE_KEY, next)
    }
  }, [])

  const value = useMemo<I18nContextValue>(
    () => ({ locale, localePref, setLocale }),
    [locale, localePref, setLocale],
  )

  // B1-14 (review P1-5 / R1-3): en merged UNDER every locale so missing keys
  // resolve to English instead of rendering the raw message id — and instead
  // of react-intl logging a console error per miss. Doing it at the provider
  // level is the only spot that covers every `formatMessage` path (useT,
  // direct useIntl, IntlProvider context consumers) in one line. Memoized
  // per locale so the merged identity stays stable between switches and
  // memoized subtrees don't re-render.
  const messages = useMemo(() => ({ ...MESSAGES.en, ...MESSAGES[locale] }), [locale])

  return (
    <IntlProvider locale={locale} defaultLocale="en" messages={messages}>
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
 * needs a translated user-facing string. Detects the persisted locale the
 * same way `I18nProvider` does (B1-14: any supported locale, not just
 * en/zh-CN); every key falls back to `en`, then to the raw id — the same
 * merge semantics the IntlProvider applies above.
 */
export function messageFor(id: string, values?: Record<string, PrimitiveType>): string {
  const locale = resolveLocale(getStoredLocalePref())
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
