import { useState } from 'react'
import { useIntl } from 'react-intl'
import { NavLink } from 'react-router-dom'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { useTheme } from '@/context/ThemeContext'
import { readReduceGlass, setReduceGlass } from '@/lib/glass'

export default function ThemeSettings() {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const { theme, setTheme, themes, fontScale, setFontScale } = useTheme()
  // 材质 (design-parity R1 2026-10-08): glass strength — Reduced applies the
  // persistent html.reduce-glass class (lib/glass), sharing the same solid
  // fill recipe as the OS prefers-reduced-transparency fallback.
  const [reduceGlass, setReduceGlassState] = useState(readReduceGlass)
  const handleReduceGlass = (reduced: boolean) => {
    setReduceGlassState(reduced)
    setReduceGlass(reduced)
  }

  const fontSizes = [
    { value: 0.85, label: t('settings.theme.fontSize.small') },
    { value: 1.0, label: t('settings.theme.fontSize.medium') },
    { value: 1.15, label: t('settings.theme.fontSize.large') },
    { value: 1.3, label: t('settings.theme.fontSize.xlarge') },
  ]

  return (
    <div className="max-w-narrow">
      <p className="font-body-md text-on-surface-variant mb-md">{t('settings.theme.subtitle')}</p>

      <div className="space-y-lg pb-10">
        {/* Theme Selection */}
        <section className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-e1">
          <h3 className="font-headline-md text-headline-md mb-md">{t('settings.theme.themeLabel')}</h3>
          <div className="grid grid-cols-2 md:grid-cols-4 gap-md">
            {themes.map(opt => (
              <Button
                key={opt.id}
                variant="outline"
                onClick={() => setTheme(opt.id)}
                className={cn(
                  'h-auto cursor-pointer p-md rounded-xl border-2 transition-all text-left whitespace-normal',
                  theme === opt.id
                    ? 'border-primary bg-primary-fixed/20 shadow-e1'
                    : 'border-outline-variant/30 hover:border-primary/50',
                )}
              >
                <div className="aspect-video rounded-md mb-sm border border-outline-variant/20 overflow-hidden bg-background p-xs space-y-xs">
                  <div className="flex items-center gap-xs mb-xs">
                    <div className="w-3 h-3 rounded-sm bg-primary-container" />
                    <div className="h-1 flex-1 bg-outline-variant/20 rounded-sm" />
                  </div>
                  <div className="flex justify-end">
                    <div className="bg-primary rounded-sm px-xs py-[1px] max-w-[60%]">
                      <div className="h-1 bg-on-primary/50 rounded-sm w-8" />
                    </div>
                  </div>
                  <div className="flex gap-xs">
                    <div className="w-3 h-3 rounded-full bg-primary-container shrink-0" />
                    <div className="bg-surface-container-lowest border border-outline-variant/10 rounded-sm px-xs py-[1px] max-w-[70%]">
                      <div className="h-1 bg-on-surface-variant/30 rounded-sm w-10" />
                    </div>
                  </div>
                  <div className="flex justify-end">
                    <div className="bg-primary rounded-sm px-xs py-[1px] max-w-[45%]">
                      <div className="h-1 bg-on-primary/50 rounded-sm w-5" />
                    </div>
                  </div>
                </div>
                <p className={cn('text-center font-label-md', theme === opt.id ? 'text-on-surface font-bold' : 'text-on-surface')}>
                  {opt.id === 'system' && <span className="material-symbols-outlined icon-sm align-middle mr-xs">monitor</span>}
                  {t(opt.labelKey)}
                </p>
              </Button>
            ))}
          </div>
        </section>

        {/* Font Size Selection */}
        <section className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-e1">
          <h3 className="font-headline-md text-headline-md mb-md">{t('settings.theme.fontSize.title')}</h3>
          <p className="font-body-sm text-on-surface-variant mb-lg">{t('settings.theme.fontSize.subtitle')}</p>

          <div className="flex gap-md mb-lg">
            {fontSizes.map(size => (
              <Button
                key={size.value}
                variant="outline"
                onClick={() => setFontScale(size.value)}
                className={cn(
                  'flex-1 py-md px-sm rounded-lg border-2 transition-all font-label-md',
                  Math.abs(fontScale - size.value) < 0.01
                    ? 'border-primary bg-primary-fixed/30 shadow-e1'
                    : 'border-outline-variant/30 hover:border-primary/50',
                )}
              >
                {size.label}
              </Button>
            ))}
          </div>

          {/* Live Preview */}
          <div className="bg-surface-container-low rounded-lg p-md border border-outline-variant/20">
            <p className="font-body-md text-on-surface">{t('settings.theme.fontSize.preview')}</p>
          </div>
        </section>

        {/* 材质 (design 12-settings-appearance.html §d, parity R1 2026-10-08):
            glass strength segmented control + pointers to the terminal
            knobs. The 代码字体/终端配色 switches live in the advanced page's
            terminal card — linked, not migrated (shared tests + engine
            config contract). */}
        <section className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-e1" data-testid="theme-material-card">
          <h3 className="font-headline-md text-headline-md mb-md">{t('settings.theme.material.title')}</h3>
          <p className="font-label-md text-on-surface mb-sm">{t('settings.theme.glass.label')}</p>
          <div role="radiogroup" aria-label={t('settings.theme.glass.label')} data-testid="theme-glass-group">
            <div className="flex rounded-xl bg-surface-container-low p-xs gap-xs border border-outline-variant/30 max-w-sm">
              {([
                { reduced: false, labelKey: 'settings.theme.glass.standard' },
                { reduced: true, labelKey: 'settings.theme.glass.reduced' },
              ]).map(opt => (
                <button
                  key={opt.labelKey}
                  type="button"
                  role="radio"
                  aria-checked={reduceGlass === opt.reduced}
                  data-testid={`theme-glass-${opt.reduced ? 'reduced' : 'standard'}`}
                  onClick={() => handleReduceGlass(opt.reduced)}
                  className={cn(
                    'flex-1 min-w-0 px-md py-sm rounded-lg font-label-md text-center cursor-pointer transition-all duration-(--duration-normal)',
                    'focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary',
                    reduceGlass === opt.reduced
                      ? 'bg-primary text-on-primary font-bold shadow-e1'
                      : 'text-on-surface-variant hover:text-primary hover:bg-surface-container-high',
                  )}
                >
                  {t(opt.labelKey)}
                </button>
              ))}
            </div>
            <p className="font-body-sm text-on-surface-variant mt-sm px-xs">
              {t('settings.theme.glass.hint')}
            </p>
          </div>
          {/* Pointers to the advanced page's terminal card — honest links,
              not duplicated controls. */}
          <div className="mt-md border-t border-outline-variant/20 pt-md space-y-xs">
            {([
              { labelKey: 'settings.theme.codeFont.label', icon: 'code' },
              { labelKey: 'settings.theme.terminalTheme.label', icon: 'terminal' },
            ]).map(row => (
              <div key={row.labelKey} className="flex items-center gap-sm px-xs py-xs">
                <span className="material-symbols-outlined icon-md text-on-surface-variant" aria-hidden="true">{row.icon}</span>
                <p className="flex-1 font-label-md text-on-surface">{t(row.labelKey)}</p>
                <NavLink
                  to="/settings/advanced"
                  className="flex items-center gap-xs text-link font-label-md text-body-sm hover:underline cursor-pointer whitespace-nowrap"
                >
                  {t('settings.theme.material.editInAdvanced')}
                  <span className="material-symbols-outlined icon-sm" aria-hidden="true">arrow_forward</span>
                </NavLink>
              </div>
            ))}
          </div>
        </section>

        {/* Active Theme Info */}
        <section className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-e1">
          <div className="flex items-center justify-between">
            <div>
              <h3 className="font-headline-md text-headline-md">{t('settings.theme.activeTheme')}</h3>
              <p className="font-body-sm text-on-surface-variant mt-xs">{intl.formatMessage({ id: 'settings.theme.usingTheme' }, { theme: t(themes.find(opt => opt.id === theme)?.labelKey ?? 'settings.theme.name.system') })}</p>
            </div>
            <div className="flex gap-sm">
              <div className="w-8 h-8 rounded-full bg-primary ring-2 ring-primary/30" title={t('settings.theme.colorPrimary')} />
              <div className="w-8 h-8 rounded-full bg-secondary ring-2 ring-secondary/30" title={t('settings.theme.colorSecondary')} />
              <div className="w-8 h-8 rounded-full bg-tertiary ring-2 ring-tertiary/30" title={t('settings.theme.colorTertiary')} />
            </div>
          </div>
        </section>
      </div>
    </div>
  )
}
