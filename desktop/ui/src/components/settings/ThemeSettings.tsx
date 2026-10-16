import { useState } from 'react'
import { useIntl } from 'react-intl'
import { NavLink } from 'react-router-dom'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { useTheme } from '@/context/ThemeContext'
import { readReduceGlass, setReduceGlass } from '@/lib/glass'
import { THEME_PALETTES } from '@/theme/generated/theme-palettes'

export default function ThemeSettings() {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  // H1: `theme` is a state readout — the only entry is app-defined and the
  // card below is not a picker, so `setTheme` has no caller on this page.
  const { theme, themes, fontScale, setFontScale } = useTheme()
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

      <div className="space-y-3 pb-10">
        {/* Theme Selection — H1 theme convergence (2026-10-17 ruling): the
            grid collapsed to ONE card. Aurora Ink is the app-defined theme
            (ThemeContext normalizes any stored value to it), so the card is a
            state marker, not a picker: no button, no hover affordance, no
            setTheme call — clicking is a no-op by construction. G4a's REAL
            two-tone swatch (theme background | primary, generated from
            scripts/theme-source.json via src/theme/generated/theme-palettes.ts;
            re-run `pnpm generate:themes` after re-tuning, the generator
            refuses drift) and G1-B's ✓ current chip are kept. The muted line
            under the card states the contract. */}
        <section className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-4 shadow-e1">
          <h3 className="text-label-lg font-semibold text-on-surface mb-sm">{t('settings.theme.themeLabel')}</h3>
          <div className="grid grid-cols-2 md:grid-cols-4 gap-md">
            {themes.map(opt => {
              const palette = THEME_PALETTES[opt.id]
              return (
              <div
                key={opt.id}
                aria-current="true"
                data-testid={`theme-card-${opt.id}`}
                className="relative h-auto p-md rounded-xl border border-primary text-left whitespace-normal"
              >
                <span
                  className="absolute top-1.5 right-1.5 inline-flex items-center gap-0.5 px-1.5 py-[1px] rounded-full bg-success-container text-on-success-container text-label-2xs font-bold"
                  title={t('palette.item.current')}
                >
                  <span className="material-symbols-outlined" style={{ fontSize: 12 }} aria-hidden="true">check</span>
                  {t('palette.item.current')}
                </span>
                <div
                  data-testid={`theme-swatch-${opt.id}`}
                  title={
                    palette
                      ? `${t(opt.labelKey)} · ${palette.bg} / ${palette.accent}`
                      : t(opt.labelKey)
                  }
                  className="h-10 rounded-md mb-sm border border-outline-variant/20 overflow-hidden flex"
                >
                  {/* Left = theme background, right = theme accent (the
                      design's two-tone bar). */}
                  <div
                    className="flex-1"
                    style={{ background: palette ? palette.bg : 'var(--color-surface-container)' }}
                  />
                  <div
                    className="flex-1"
                    style={{ background: palette ? palette.accent : 'var(--color-primary)' }}
                  />
                </div>
                <p className="text-center font-label-md text-on-surface font-bold">
                  {t(opt.labelKey)}
                </p>
              </div>
              )
            })}
          </div>
          <p className="font-body-sm text-on-surface-variant mt-sm px-xs">
            {t('settings.theme.definedByApp')}
          </p>
        </section>

        {/* Font Size Selection */}
        <section className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-4 shadow-e1">
          <h3 className="text-label-lg font-semibold text-on-surface mb-sm">{t('settings.theme.fontSize.title')}</h3>
          <p className="font-body-sm text-on-surface-variant mb-sm">{t('settings.theme.fontSize.subtitle')}</p>

          {/* G1-B: selection = 1px violet outline, fill unchanged. */}
          <div className="flex gap-md mb-lg">
            {fontSizes.map(size => (
              <Button
                key={size.value}
                variant="outline"
                onClick={() => setFontScale(size.value)}
                aria-pressed={Math.abs(fontScale - size.value) < 0.01}
                className={cn(
                  'flex-1 py-md px-sm rounded-lg border transition-all font-label-md',
                  Math.abs(fontScale - size.value) < 0.01
                    ? 'border-primary'
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
        <section className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-4 shadow-e1" data-testid="theme-material-card">
          <h3 className="text-label-lg font-semibold text-on-surface mb-sm">{t('settings.theme.material.title')}</h3>
          <p className="font-label-md text-on-surface mb-sm">{t('settings.theme.glass.label')}</p>
          <div role="radiogroup" aria-label={t('settings.theme.glass.label')} data-testid="theme-glass-group">
            {/* G1-B S1 (设计 `.seg`): container seg, violet dim selected. */}
            <div className="flex rounded-[9px] bg-surface-container-low p-[2px] gap-[2px] border border-outline-variant/30 max-w-sm">
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
                    'flex-1 min-w-0 px-md py-sm rounded-[7px] font-label-md text-center cursor-pointer transition-all duration-(--duration-normal)',
                    'focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary',
                    reduceGlass === opt.reduced
                      ? 'bg-primary-container text-on-primary-container font-semibold'
                      : 'text-on-surface-variant hover:text-on-surface',
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
        <section className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-4 shadow-e1">
          <div className="flex items-center justify-between">
            <div>
              <h3 className="text-label-lg font-semibold text-on-surface">{t('settings.theme.activeTheme')}</h3>
              <p className="font-body-sm text-on-surface-variant mt-xs">{intl.formatMessage({ id: 'settings.theme.usingTheme' }, { theme: t(themes.find(opt => opt.id === theme)?.labelKey ?? 'settings.theme.name.auroraInk') })}</p>
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
