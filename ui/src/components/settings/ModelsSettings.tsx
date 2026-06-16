import { useState } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { useApp } from '@/context/AppContext'
import * as api from '@/lib/tauri-api'

export default function ModelsSettings() {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const { models, status, config, refreshModels, refreshStatus } = useApp()
  const [switching, setSwitching] = useState<string | null>(null)
  const [strategy, setStrategyState] = useState<'speed' | 'balanced' | 'high-quality'>(
    (config?.performance_strategy as 'speed' | 'balanced' | 'high-quality') ?? 'high-quality'
  )

  const setStrategy = (s: 'speed' | 'balanced' | 'high-quality') => {
    setStrategyState(s)
    api.configure({ key: 'performance_strategy', value: s }).then(() => toast.success(intl.formatMessage({ id: 'settings.models.strategySet' }, { strategy: s }))).catch(e => { console.warn('ModelsSettings error:', e); toast.error(t('settings.models.strategyFailed')) })
  }
  const [showKey, setShowKey] = useState(false)

  const handleModelSwitch = async (modelId: string) => {
    if (!status) return
    setSwitching(modelId)
    try {
      await api.switchProvider({ provider: status.provider, model: modelId })
      await Promise.all([refreshModels(), refreshStatus()])
      toast.success(intl.formatMessage({ id: 'settings.models.switched' }, { model: modelId }))
    } catch (e) { console.warn("ModelsSettings error:", e); toast.error(t('settings.models.switchFailed')) }
    setSwitching(null)
  }

  const currentModel = status?.model
  const providers = [...new Set(models.map(m => m.provider))]
  const [activeProvider, setActiveProvider] = useState<string | null>(null)
  const filteredModels = activeProvider ? models.filter(m => m.provider === activeProvider) : models

  return (
    <div className="max-w-[1200px] pr-8 pb-10">
      <header className="mb-md">
        <h2 className="font-headline-lg text-headline-lg text-on-surface mb-xs">{t('settings.models.title')}</h2>
        <p className="font-body-md text-on-surface-variant">{t('settings.models.subtitle')}</p>
      </header>

      <div className="space-y-lg">
        {/* Performance Strategy */}
        <section className="bg-surface-container-lowest border border-outline-variant/30 rounded-xl p-lg shadow-sm">
          <h3 className="font-headline-md text-on-surface mb-md">{t('settings.models.perfStrategy')}</h3>
          <div className="flex bg-surface-container-low p-xs rounded-xl gap-xs max-w-2xl">
            {(['balanced', 'speed', 'high-quality'] as const).map(s => (
              <button
                key={s}
                onClick={() => setStrategy(s)}
                className={`flex-1 py-sm font-label-md rounded-lg transition-all cursor-pointer ${
                  strategy === s
                    ? 'bg-surface-container-lowest text-primary shadow-sm ring-1 ring-black/5 font-bold'
                    : 'text-on-surface-variant hover:bg-surface-container-high'
                }`}
              >
                {s.charAt(0).toUpperCase() + s.slice(1).replace('-', ' ')}
              </button>
            ))}
          </div>
          <p className="mt-md text-label-sm text-on-surface-variant opacity-70 flex items-center gap-xs">
            <span className="material-symbols-outlined text-[16px]">info</span>
            {strategy === 'high-quality' ? t('settings.models.stratHighQuality') : strategy === 'speed' ? t('settings.models.stratSpeed') : t('settings.models.stratBalanced')}
          </p>
        </section>

        {/* Active Model */}
        <section className="bg-surface-container-lowest border border-outline-variant/30 rounded-xl p-lg shadow-sm">
          <h3 className="font-headline-md text-on-surface mb-md">{t('settings.models.activeModel')}</h3>
          {currentModel ? (
            <div className="p-md rounded-xl border-2 border-primary bg-primary-container/5 flex items-center justify-between transition-all">
              <div className="flex items-center gap-md">
                <div className="w-10 h-10 rounded-lg bg-primary text-on-primary flex items-center justify-center">
                  <span className="material-symbols-outlined">auto_awesome</span>
                </div>
                <div>
                  <div className="flex items-center gap-xs">
                    <span className="font-headline-md text-primary text-lg">{currentModel}</span>
                    <span className="px-xs py-[2px] bg-primary text-on-primary rounded text-[10px] font-bold">{t('settings.models.activeBadge')}</span>
                  </div>
                  <p className="text-label-sm text-on-surface-variant opacity-70">{intl.formatMessage({ id: 'settings.models.providerLabel' }, { provider: status?.provider })}</p>
                </div>
              </div>
            </div>
          ) : (
            <p className="text-body-sm text-on-surface-variant">{t('settings.models.noModelSelected')}</p>
          )}
        </section>

        {/* Provider Tabs */}
        <section className="bg-surface-container-lowest border border-outline-variant/30 rounded-xl shadow-sm overflow-hidden">
          <div className="border-b border-outline-variant/30 bg-surface-container-low/30 px-lg pt-md">
            <div className="flex gap-lg overflow-x-auto">
              <button
                onClick={() => setActiveProvider(null)}
                className={`pb-sm px-xs border-b-2 font-label-md whitespace-nowrap cursor-pointer transition-colors ${!activeProvider ? 'border-primary text-primary font-bold' : 'border-transparent text-on-surface-variant hover:text-primary'}`}
              >{t('settings.models.tabAll')}</button>
              {providers.map(p => (
                <button
                  key={p}
                  onClick={() => setActiveProvider(activeProvider === p ? null : p)}
                  className={`pb-sm px-xs border-b-2 font-label-md whitespace-nowrap cursor-pointer transition-colors ${activeProvider === p ? 'border-primary text-primary font-bold' : 'border-transparent text-on-surface-variant hover:text-primary'}`}
                >{p}</button>
              ))}
              {providers.length === 0 && <span className="pb-sm px-xs text-on-surface-variant font-label-md">{t('settings.models.noProviders')}</span>}
            </div>
          </div>

          <div className="p-lg">
            <div className="flex justify-between items-center mb-lg">
              <div>
                <h3 className="font-headline-md text-on-surface">{t('settings.models.availableModels')}</h3>
                <p className="text-body-sm text-on-surface-variant">{t('settings.models.availableDesc')}</p>
              </div>
              <span className="inline-flex items-center px-sm py-1 bg-primary/10 text-primary rounded-full text-[10px] font-bold tracking-wider uppercase">
                {intl.formatMessage({ id: 'settings.models.count' }, { count: models.length })}
              </span>
            </div>

            {filteredModels.length === 0 ? (
              <p className="text-body-sm text-on-surface-variant py-lg text-center">{t('settings.models.noModelsFound')}</p>
            ) : (
              <div className="grid grid-cols-1 gap-md">
                {filteredModels.map(m => (
                  <button
                    key={m.id}
                    onClick={() => handleModelSwitch(m.id)}
                    disabled={switching !== null}
                    className={`p-md rounded-xl border flex items-center justify-between hover:border-primary/50 transition-all group cursor-pointer text-left w-full ${
                      m.id === currentModel ? 'border-2 border-primary bg-primary-container/5' : 'border-outline-variant/50'
                    }`}
                  >
                    <div className="flex items-center gap-md">
                      <div className={`w-10 h-10 rounded-lg flex items-center justify-center ${
                        m.id === currentModel ? 'bg-primary text-on-primary' : 'bg-surface-container-high text-on-surface-variant'
                      }`}>
                        <span className="material-symbols-outlined">psychology</span>
                      </div>
                      <div>
                        <div className="flex items-center gap-xs">
                          <span className={`font-headline-md text-lg ${m.id === currentModel ? 'text-primary' : 'text-on-surface'}`}>{m.name}</span>
                          {m.id === currentModel ? <span className="px-xs py-[2px] bg-primary text-on-primary rounded text-[10px] font-bold">{t('settings.models.defaultBadge')}</span> : null}
                        </div>
                        <p className="text-label-sm text-on-surface-variant opacity-70">{m.provider} {m.context_window > 0 ? `· ${(m.context_window / 1000).toFixed(0)}k context` : ''}</p>
                      </div>
                    </div>
                    {switching === m.id ? (
                      <span className="material-symbols-outlined text-primary animate-spin text-[20px]">progress_activity</span>
                    ) : null}
                  </button>
                ))}
              </div>
            )}
          </div>

          {/* API Key */}
          <div className="bg-surface-container-low/50 p-lg border-t border-outline-variant/30">
            <div className="flex items-center gap-sm mb-md">
              <span className="material-symbols-outlined text-primary">key</span>
              <h4 className="font-label-md font-bold text-on-surface">{intl.formatMessage({ id: 'settings.models.apiConnection' }, { provider: (config?.provider ?? t('settings.models.providerDefault')) })}</h4>
            </div>
            <div className="flex gap-md max-w-xl">
              <div className="relative flex-1">
                <Input className="w-full px-md py-sm bg-surface text-on-surface border border-outline-variant/50 rounded-lg focus:ring-2 focus:ring-primary outline-none transition-all font-body-sm pr-10" type={showKey ? 'text' : 'password'} value={config?.api_key ? 'sk-••••••••••••' : ''} readOnly />
                <Button variant="ghost" className="absolute right-2 top-1/2 -translate-y-1/2 text-on-surface-variant hover:text-primary cursor-pointer" onClick={() => setShowKey(v => !v)}>
                  <span className="material-symbols-outlined text-[20px]">{showKey ? 'visibility_off' : 'visibility'}</span>
                </Button>
              </div>
              <Button
                className="px-lg py-sm border border-outline-variant bg-surface-container-lowest text-on-surface font-label-md rounded-lg hover:bg-surface-container transition-colors flex items-center gap-sm whitespace-nowrap cursor-pointer"
                onClick={() => refreshModels()}
              >
                <span className="material-symbols-outlined text-[18px]">sync</span>
                {t('settings.models.refresh')}
              </Button>
            </div>
          </div>
        </section>

        {/* Global Parameters */}
        <section className="bg-surface-container-lowest border border-outline-variant/30 rounded-xl p-lg shadow-sm">
          <h3 className="font-headline-md text-on-surface mb-lg">{t('settings.models.globalParams')}</h3>
          <p className="text-body-sm text-on-surface-variant mb-xl -mt-md">{t('settings.models.globalParamsDesc')}</p>
          <div className="space-y-xl max-w-2xl">
            <ParameterSlider label={t('settings.models.temperature')} value={0.7} min={0} max={1} step={0.1} lowLabel={t('settings.models.precise')} highLabel={t('settings.models.creative')} configKey="temperature" />
            <ParameterSlider label={t('settings.models.maxTokens')} value={4096} min={256} max={128000} step={256} formatValue={v => v >= 1000 ? `${(v / 1000).toFixed(0)}k` : String(v)} lowLabel={t('settings.models.short')} highLabel={t('settings.models.longContext')} configKey="max_tokens" />
          </div>
        </section>
      </div>
    </div>
  )
}

function ParameterSlider({ label, value: initialValue, min, max, step, formatValue, lowLabel, highLabel, configKey }: {
  label: string
  value: number
  min: number
  max: number
  step: number
  formatValue?: (v: number) => string
  lowLabel?: string
  highLabel?: string
  configKey?: string
}) {
  const [value, setValue] = useState(initialValue)
  const display = formatValue ? formatValue(value) : String(value)

  const handleChange = (newValue: number) => {
    setValue(newValue)
    if (configKey) {
      api.configure({ key: configKey, value: String(newValue) }).catch(e => console.warn('ParameterSlider error:', e))
    }
  }

  return (
    <div>
      <div className="flex justify-between items-center mb-sm">
        <label className="font-label-md text-on-surface-variant">{label}</label>
        <span className="font-label-sm text-primary bg-primary-container/20 px-sm py-xs rounded">{display}</span>
      </div>
      <input
        className="w-full appearance-none bg-outline-variant/30 h-1 rounded-full cursor-pointer outline-none slider-thumb-primary"
        min={min} max={max} step={step} type="range" value={value}
        onChange={e => handleChange(Number(e.target.value))}
      />
      {lowLabel && highLabel ? (
        <div className="flex justify-between mt-xs">
          <span className="font-label-sm text-on-surface-variant opacity-50">{lowLabel}</span>
          <span className="font-label-sm text-on-surface-variant opacity-50">{highLabel}</span>
        </div>
      ) : null}
    </div>
  )
}
