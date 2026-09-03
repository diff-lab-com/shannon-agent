import { useEffect, useState } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { useCatalog } from '@/context/CatalogContext'
import * as api from '@/lib/tauri-api'
import { toastError } from '@/lib/errorToast'
import { cn } from '@/lib/utils'
import type { ProvidersFile } from '@/types'
import { formatPrice } from './models-settings/types'
import { ProvidersSection } from './models-settings/ProvidersSection'
import { ProviderVisibilitySection } from './models-settings/ProviderVisibilitySection'
import { ParameterSlider } from './models-settings/ParameterSlider'

export default function ModelsSettings() {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const { models, status, config, refreshModels, refreshStatus } = useCatalog()
  const [switching, setSwitching] = useState<string | null>(null)
  const [strategy, setStrategyState] = useState<'speed' | 'balanced' | 'high-quality'>(
    (config?.performance_strategy as 'speed' | 'balanced' | 'high-quality') ?? 'high-quality'
  )

  // Managed providers (Models P2). Loaded once on mount; mutations update
  // local state from each command's returned (masked) file.
  const [providersFile, setProvidersFile] = useState<ProvidersFile>({
    active_provider_id: null,
    providers: [],
  })
  const [loadingProviders, setLoadingProviders] = useState(true)

  useEffect(() => {
    let cancelled = false
    api.listProviders()
      .then((f) => { if (!cancelled) setProvidersFile(f) })
      .catch((e) => console.warn('listProviders error:', e))
      .finally(() => { if (!cancelled) setLoadingProviders(false) })
    return () => { cancelled = true }
  }, [])

  const setStrategy = (s: 'speed' | 'balanced' | 'high-quality') => {
    setStrategyState(s)
    api.configure({ key: 'performance_strategy', value: s }).then(() => toast.success(intl.formatMessage({ id: 'settings.models.strategySet' }, { strategy: s }))).catch((e) => { toastError(t('settings.models.strategyFailed'), e) })
  }

  const handleModelSwitch = async (modelId: string) => {
    if (!status) return
    setSwitching(modelId)
    try {
      await api.configure({ key: 'model', value: modelId })
      await Promise.all([refreshModels(), refreshStatus()])
      toast.success(intl.formatMessage({ id: 'settings.models.switched' }, { model: modelId }))
    } catch (e) { toastError(t('settings.models.switchFailed'), e) }
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
              <Button
                key={s}
                variant="ghost"
                onClick={() => setStrategy(s)}
                className={cn(
                  'flex-1 py-sm font-label-md rounded-lg transition-all cursor-pointer',
                  strategy === s
                    ? 'bg-surface-container-lowest text-primary shadow-sm ring-1 ring-black/5 font-bold'
                    : 'text-on-surface-variant hover:bg-surface-container-high',
                )}
              >
                {s === 'high-quality' ? t('settings.models.stratLabel.highQuality') : s === 'speed' ? t('settings.models.stratLabel.speed') : t('settings.models.stratLabel.balanced')}
              </Button>
            ))}
          </div>
          <p className="mt-md text-label-sm text-on-surface-variant flex items-center gap-xs">
            <span className="material-symbols-outlined icon-sm">info</span>
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
                  <p className="text-label-sm text-on-surface-variant">{intl.formatMessage({ id: 'settings.models.providerLabel' }, { provider: status?.provider })}</p>
                </div>
              </div>
            </div>
          ) : (
            <p className="text-body-sm text-on-surface-variant">{t('settings.models.noModelSelected')}</p>
          )}
        </section>

        {/* Providers (managed, Models P2) */}
        <ProvidersSection
          providersFile={providersFile}
          loading={loadingProviders}
          onChange={setProvidersFile}
          onActivated={async () => { await Promise.all([refreshModels(), refreshStatus()]) }}
        />

        {/* Provider visibility (ADR-0005 P4.9) */}
        <ProviderVisibilitySection
          onChanged={async () => { await refreshModels() }}
        />

        {/* Provider Tabs */}
        <section className="bg-surface-container-lowest border border-outline-variant/30 rounded-xl shadow-sm overflow-hidden">
          <div className="border-b border-outline-variant/30 bg-surface-container-low/30 px-lg pt-md">
            <div className="flex gap-lg overflow-x-auto">
              <Button
                variant="ghost"
                onClick={() => setActiveProvider(null)}
                className={cn(
                  'h-auto pb-sm px-xs border-b-2 font-label-md whitespace-nowrap cursor-pointer transition-colors rounded-none',
                  !activeProvider ? 'border-primary text-primary font-bold' : 'border-transparent text-on-surface-variant hover:text-primary',
                )}
              >{t('settings.models.tabAll')}</Button>
              {providers.map(p => (
                <Button
                  key={p}
                  variant="ghost"
                  onClick={() => setActiveProvider(activeProvider === p ? null : p)}
                  className={cn(
                    'h-auto pb-sm px-xs border-b-2 font-label-md whitespace-nowrap cursor-pointer transition-colors rounded-none',
                    activeProvider === p ? 'border-primary text-primary font-bold' : 'border-transparent text-on-surface-variant hover:text-primary',
                  )}
                >{p}</Button>
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
                  <Button
                    key={m.id}
                    variant="outline"
                    onClick={() => handleModelSwitch(m.id)}
                    disabled={switching !== null}
                    className={cn(
                      'h-auto p-md rounded-xl border flex items-center justify-between hover:border-primary/50 transition-all group cursor-pointer text-left w-full whitespace-normal',
                      m.id === currentModel ? 'border-2 border-primary bg-primary-container/5' : 'border-outline-variant/50',
                    )}
                  >
                    <div className="flex items-center gap-md">
                      <div className={cn("w-10 h-10 rounded-lg flex items-center justify-center",
                        m.id === currentModel ? 'bg-primary text-on-primary' : 'bg-surface-container-high text-on-surface-variant',
                      )}>
                        <span className="material-symbols-outlined">psychology</span>
                      </div>
                      <div>
                    <div className="flex items-center gap-xs">
                      <span className={cn("font-headline-md text-lg", m.id === currentModel ? 'text-primary' : 'text-on-surface')}>{m.name}</span>
                      {m.id === currentModel ? <span className="px-xs py-[2px] bg-primary text-on-primary rounded text-[10px] font-bold">{t('settings.models.defaultBadge')}</span> : null}
                      {m.tier ? (
                        <span
                          className="px-xs py-[2px] bg-secondary-container text-on-secondary-container rounded text-[10px] font-bold uppercase tracking-wider"
                          title={t('settings.models.tier')}
                        >
                          {t(`settings.models.tier${m.tier.charAt(0).toUpperCase()}${m.tier.slice(1)}` as 'settings.models.tierFast' | 'settings.models.tierStandard' | 'settings.models.tierPro')}
                        </span>
                      ) : null}
                      {m.dynamic ? (
                        <span
                          className="px-xs py-[2px] bg-tertiary-container text-on-tertiary-container rounded text-[10px] font-bold uppercase tracking-wider"
                          title={t('models.modelsDev.title')}
                        >
                          {t('settings.models.dynamicBadge')}
                        </span>
                      ) : null}
                    </div>
                    <p className="text-label-sm text-on-surface-variant">
                      {m.provider}
                      {m.context_window > 0
                        ? ' ' + intl.formatMessage({ id: 'settings.models.contextWindow' }, { count: (m.context_window / 1000).toFixed(0) })
                        : ''}
                      {' · '}
                      {intl.formatMessage(
                        { id: 'settings.models.priceInput' },
                        { value: formatPrice(m.price_in) },
                      )}
                      {' / '}
                      {intl.formatMessage(
                        { id: 'settings.models.priceOutput' },
                        { value: formatPrice(m.price_out) },
                      )}
                    </p>
                  </div>
                    </div>
                    {switching === m.id ? (
                      <span className="material-symbols-outlined text-primary animate-spin text-[20px]">progress_activity</span>
                    ) : null}
                  </Button>
                ))}
              </div>
            )}
          </div>
        </section>

        {/* Global Parameters */}
        <section className="bg-surface-container-lowest border border-outline-variant/30 rounded-xl p-lg shadow-sm">
          <h3 className="font-headline-md text-on-surface mb-lg">{t('settings.models.globalParams')}</h3>
          <p className="text-body-sm text-on-surface-variant mb-xl -mt-md">{t('settings.models.globalParamsDesc')}</p>
          <div className="space-y-xl max-w-2xl">
            <ParameterSlider label={t('settings.models.temperature')} value={config?.temperature ?? 0.7} min={0} max={1} step={0.1} lowLabel={t('settings.models.precise')} highLabel={t('settings.models.creative')} configKey="temperature" />
            <ParameterSlider label={t('settings.models.maxTokens')} value={config?.max_tokens ?? 4096} min={256} max={128000} step={256} formatValue={v => v >= 1000 ? `${(v / 1000).toFixed(0)}k` : String(v)} lowLabel={t('settings.models.short')} highLabel={t('settings.models.longContext')} configKey="max_tokens" />
          </div>
        </section>
      </div>
    </div>
  )
}