import { useTheme } from '@/context/ThemeContext'

export default function ThemeSettings() {
  const { theme, setTheme, themes } = useTheme()

  return (
    <div className="max-w-3xl">
      <header className="mb-xl">
        <h2 className="font-headline-lg text-headline-lg text-on-surface mb-xs">Theme Settings</h2>
        <p className="font-body-md text-on-surface-variant">Customize the visual environment to match your cognitive workflow.</p>
      </header>

      <div className="space-y-lg pb-10">
        {/* Theme Selection */}
        <section className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-sm">
          <h3 className="font-headline-md text-headline-md mb-md">Theme</h3>
          <div className="grid grid-cols-2 md:grid-cols-4 gap-md">
            {themes.map(t => (
              <button
                key={t.id}
                onClick={() => setTheme(t.id)}
                className={`cursor-pointer p-md rounded-xl border-2 transition-all text-left ${
                  theme === t.id
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
                <p className={`text-center font-label-md ${theme === t.id ? 'text-primary font-bold' : 'text-on-surface'}`}>
	                  {t.id === 'system' && <span className="material-symbols-outlined text-[14px] align-middle mr-xs">monitor</span>}
	                  {t.label}
	                </p>
              </button>
            ))}
          </div>
        </section>

        {/* Active Theme Info */}
        <section className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-sm">
          <div className="flex items-center justify-between">
            <div>
              <h3 className="font-headline-md text-headline-md">Active Theme</h3>
              <p className="font-body-sm text-on-surface-variant mt-xs">Currently using <strong className="text-primary">{themes.find(t => t.id === theme)?.label ?? theme}</strong> theme.</p>
            </div>
            <div className="flex gap-sm">
              <div className="w-8 h-8 rounded-full bg-primary ring-2 ring-primary/30" title="Primary" />
              <div className="w-8 h-8 rounded-full bg-secondary ring-2 ring-secondary/30" title="Secondary" />
              <div className="w-8 h-8 rounded-full bg-tertiary ring-2 ring-tertiary/30" title="Tertiary" />
            </div>
          </div>
        </section>
      </div>
    </div>
  )
}
