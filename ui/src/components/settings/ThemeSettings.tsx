import { useIntl } from 'react-intl'
import { useTheme } from '@/context/ThemeContext'

export default function ThemeSettings() {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const { theme, setTheme, themes, fontScale, setFontScale } = useTheme()

  const fontSizes = [
    { value: 0.85, label: t('settings.theme.fontSize.small') },
    { value: 1.0, label: t('settings.theme.fontSize.medium') },
    { value: 1.15, label: t('settings.theme.fontSize.large') },
    { value: 1.3, label: t('settings.theme.fontSize.xlarge') },
  ]

  return (
    <div className="max-w-3xl">
      <header className="mb-xl">
        <h2 className="font-headline-lg text-headline-lg text-on-surface mb-xs">{t('settings.theme.title')}</h2>
        <p className="font-body-md text-on-surface-variant">{t('settings.theme.subtitle')}</p>
      </header>

      <div className="space-y-lg pb-10">
        {/* Theme Selection */}
        <section className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-sm">
          <h3 className="font-headline-md text-headline-md mb-md">{t('settings.theme.themeLabel')}</h3>
          <div className="grid grid-cols-2 md:grid-cols-4 gap-md">
            {themes.map(opt => (
              <button
                key={opt.id}
                onClick={() => setTheme(opt.id)}
                className={`cursor-pointer p-md rounded-xl border-2 transition-all text-left ${
                  theme === opt.id
                    ? 'border-primary bg-primary-fixed/20 shadow-sm'
                    : 'border-outline-variant/30 hover:border-primary/50'
                }`}
              >
                <div className="aspect-video rounded-md mb-sm border border-outline-variant/20 overflow-hidden bg-background p-xs space-y-xs">
                  <div className="flex items-center gap-xs mb-xs">
                    <div className="w-3 h-3 rounded-sm bg-primary-container" />
                    <div className="h-1 flex-1 bg-outline-variant/20 rounded" />
                  </div>
                  <div className="flex justify-end">
                    <div className="bg-primary rounded-sm px-xs py-[1px] max-w-[60%]">
                      <div className="h-1 bg-on-primary/50 rounded w-8" />
                    </div>
                  </div>
                  <div className="flex gap-xs">
                    <div className="w-3 h-3 rounded-full bg-primary-container shrink-0" />
                    <div className="bg-surface-container-lowest border border-outline-variant/10 rounded-sm px-xs py-[1px] max-w-[70%]">
                      <div className="h-1 bg-on-surface-variant/30 rounded w-10" />
                    </div>
                  </div>
                  <div className="flex justify-end">
                    <div className="bg-primary rounded-sm px-xs py-[1px] max-w-[45%]">
                      <div className="h-1 bg-on-primary/50 rounded w-5" />
                    </div>
                  </div>
                </div>
                <p className={`text-center font-label-md ${theme === opt.id ? 'text-primary font-bold' : 'text-on-surface'}`}>
                  {opt.id === 'system' && <span className="material-symbols-outlined text-[14px] align-middle mr-xs">monitor</span>}
                  {opt.label}
                </p>
              </button>
            ))}
          </div>
        </section>

        {/* Font Size Selection */}
        <section className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-sm">
          <h3 className="font-headline-md text-headline-md mb-md">{t('settings.theme.fontSize.title')}</h3>
          <p className="font-body-sm text-on-surface-variant mb-lg">{t('settings.theme.fontSize.subtitle')}</p>

          <div className="flex gap-md mb-lg">
            {fontSizes.map(size => (
              <button
                key={size.value}
                onClick={() => setFontScale(size.value)}
                className={`flex-1 py-md px-sm rounded-lg border-2 transition-all font-label-md ${
                  Math.abs(fontScale - size.value) < 0.01
                    ? 'border-primary bg-primary-fixed/30 shadow-sm'
                    : 'border-outline-variant/30 hover:border-primary/50'
                }`}
              >
                {size.label}
              </button>
            ))}
          </div>

          {/* Live Preview */}
          <div className="bg-surface-container-low rounded-lg p-md border border-outline-variant/20">
            <p className="font-body-md text-on-surface">{t('settings.theme.fontSize.preview')}</p>
          </div>
        </section>

        {/* Active Theme Info */}
        <section className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-sm">
          <div className="flex items-center justify-between">
            <div>
              <h3 className="font-headline-md text-headline-md">{t('settings.theme.activeTheme')}</h3>
              <p className="font-body-sm text-on-surface-variant mt-xs">{intl.formatMessage({ id: 'settings.theme.usingTheme' }, { theme: (themes.find(opt => opt.id === theme)?.label ?? theme) })}</p>
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
