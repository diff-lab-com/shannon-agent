import { useEffect, useMemo, useState } from 'react'
import { Spinner } from '@/components/ui/loading-state'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { useCatalog } from '@/context/CatalogContext'
import * as api from '@/lib/tauri-api'
import { toastError } from '@/lib/errorToast'
import { cn } from '@/lib/utils'
import type { DeclaredModelInput, ProvidersFile } from '@/types'
import { formatPrice } from './models-settings/types'
import { ModelSourceBadge } from '@/components/shared/ModelPickerRow'
import { writeGlobalModelDefault } from '@/lib/modelSwitch'
import { ProvidersSection } from './models-settings/ProvidersSection'
import { ProviderVisibilitySection } from './models-settings/ProviderVisibilitySection'
import { PhaseTierSection } from './models-settings/PhaseTierSection'
import { ProfilesSection } from './models-settings/ProfilesSection'
import { ParameterSlider } from './models-settings/ParameterSlider'
import ModelMetaEditor from './models-settings/ModelMetaEditor'
import {
  activeVaultOf,
  isInVault,
  specToVaultInput,
  upsertVaultModel,
} from './models-settings/modelMeta'
import { MODEL_VAULT_SOFT_CAP } from './add-provider-modal/modelCuration'
import { useTauriEvent } from '@/hooks/useTauriEvent'
import { EVENT_NAMES } from '@/types'
import { ComboboxSelect } from '@/components/ui/combobox-select'

export default function ModelsSettings() {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const { models, status, config, refreshConfig, refreshModels, refreshStatus } = useCatalog()
  const [switching, setSwitching] = useState<string | null>(null)
  const [strategy, setStrategyState] = useState<'speed' | 'balanced' | 'high-quality'>(
    (config?.performance_strategy as 'speed' | 'balanced' | 'high-quality') ?? 'high-quality'
  )

  // P1-10: the strategy pills follow the persisted config so a failed write
  // (or an external change) snaps the selection back to what is on disk.
  useEffect(() => {
    setStrategyState((config?.performance_strategy as 'speed' | 'balanced' | 'high-quality') ?? 'high-quality')
  }, [config?.performance_strategy])

  // Managed providers (Models P2). Loaded once on mount; mutations update
  // local state from each command's returned (masked) file.
  const [providersFile, setProvidersFile] = useState<ProvidersFile>({
    active_provider_id: null,
    providers: [],
  })
  const [loadingProviders, setLoadingProviders] = useState(true)

  const reloadProviders = () => {
    return api.listProviders()
      .then((f) => setProvidersFile(f))
      .catch((e) => console.warn('listProviders error:', e))
  }

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
    api.configure({ key: 'performance_strategy', value: s })
      // B6-36: the toast used to interpolate the raw enum (`speed`) — show
      // the translated strategy label instead (same keys as the pills).
      .then(() => toast.success(intl.formatMessage(
        { id: 'settings.models.strategySet' },
        { strategy: intl.formatMessage({ id: `settings.models.stratLabel.${s}` }) },
      )))
      .catch(async (e) => {
        toastError(t('settings.models.strategyFailed'), e)
        // Revert the optimistic flip by re-reading the persisted config.
        await refreshConfig()
      })
  }

  const handleModelSwitch = async (modelId: string) => {
    if (!status) return
    // S3-1 (P-N23): resolve the catalog row and write the SAME
    // model+provider pair the Header (and the chip's global branch) write —
    // the old model-only write was the review's convention fork. Same
    // semantics (the catalog comes from the active provider), one helper.
    const model = models.find(m => m.id === modelId)
    if (!model) return
    setSwitching(modelId)
    try {
      await writeGlobalModelDefault(model)
      await Promise.all([refreshModels(), refreshStatus()])
      toast.success(intl.formatMessage({ id: 'settings.models.switched' }, { model: modelId }))
    } catch (e) { toastError(t('settings.models.switchFailed'), e) }
    setSwitching(null)
  }

  const currentModel = status?.model
  const providers = [...new Set(models.map(m => m.provider))]
  const [activeProvider, setActiveProvider] = useState<string | null>(null)
  const filteredModels = activeProvider ? models.filter(m => m.provider === activeProvider) : models

  // S2-2 (per-model metadata editor): the active provider slot's curated
  // vault. `null` when no managed slot is active (env-configured providers
  // have no vault to write to) — the row affordances stay hidden then, an
  // honest dead-end rather than a form that cannot save.
  const vault = useMemo(() => activeVaultOf(providersFile), [providersFile])
  const [expandedMeta, setExpandedMeta] = useState<string | null>(null)
  const [addingToVault, setAddingToVault] = useState<string | null>(null)
  const [savingMeta, setSavingMeta] = useState(false)

  // The editor submits the WHOLE vault (overwrite semantics), so the
  // snapshot must never go stale: a vault write from another window/CLI
  // (`provider_models`) or a provider activation that changes the vault
  // owner (`provider`) re-reads it. CONFIG_UPDATED is the same event the
  // AppContext already answers with refreshModels — this only extends it
  // to the providersFile slice this page owns.
  useTauriEvent<{ key: string; value: string }>(EVENT_NAMES.CONFIG_UPDATED, (e) => {
    if (e.payload.key === 'provider_models' || e.payload.key === 'provider') {
      void reloadProviders()
    }
  })

  // "Add to vault" on a catalog/overlay row: persist an id-only declaration
  // (the curation editor's minimal shape), then drop the user straight into
  // the metadata editor for it. `set_provider_models` replaces the vault
  // wholesale, so the payload is the full vault + the new entry.
  const handleAddToVault = async (modelId: string) => {
    if (!vault) return
    if (vault.models.length >= MODEL_VAULT_SOFT_CAP) {
      toast.warning(intl.formatMessage(
        { id: 'settings.models.vault.overCap' },
        { cap: MODEL_VAULT_SOFT_CAP },
      ))
      return
    }
    setAddingToVault(modelId)
    try {
      await api.setProviderModels(vault.providerId, [
        ...vault.models.map(specToVaultInput),
        { id: modelId },
      ])
      // The command emitted CONFIG_UPDATED, which AppContext answers with
      // refreshModels — drive both refreshes here too so the row/badge
      // updates even if this window missed the event.
      await Promise.all([refreshModels(), reloadProviders()])
      toast.success(intl.formatMessage(
        { id: 'settings.models.vault.addedToast' },
        { model: modelId },
      ))
      setExpandedMeta(modelId)
    } catch (e) {
      toastError(intl.formatMessage(
        { id: 'settings.models.vault.addFailed' },
        { model: modelId },
      ), e)
    }
    setAddingToVault(null)
  }

  // Editor save: merge the edited declaration into the vault client-side
  // (the command has NO field-merge — overwrite semantics) and submit the
  // complete vault. The editor stays open on failure so the input survives.
  const handleSaveMeta = async (input: DeclaredModelInput) => {
    if (!vault) return
    setSavingMeta(true)
    try {
      await api.setProviderModels(vault.providerId, upsertVaultModel(vault.models, input))
      await Promise.all([refreshModels(), reloadProviders()])
      toast.success(intl.formatMessage(
        { id: 'settings.models.vault.savedToast' },
        { model: input.id },
      ))
      setExpandedMeta(null)
    } catch (e) {
      toastError(intl.formatMessage(
        { id: 'settings.models.vault.saveFailed' },
        { model: input.id },
      ), e)
    }
    setSavingMeta(false)
  }

  // R2-2: manual models.dev overlay refresh (same engine path as the CLI
  // `/model refresh`). Idle → busy (spinner) → done (model count) / failed
  // (inline reason). The overlay feeds `list_models`, so a success also
  // refreshes the visible catalog.
  const [catalogRefresh, setCatalogRefresh] = useState<
    { phase: 'idle' } | { phase: 'busy' } | { phase: 'done'; count: number } | { phase: 'failed'; reason: string }
  >({ phase: 'idle' })

  const handleRefreshCatalog = async () => {
    setCatalogRefresh({ phase: 'busy' })
    try {
      const r = await api.refreshModelCatalog()
      setCatalogRefresh({ phase: 'done', count: r.count })
      await refreshModels()
    } catch (e) {
      setCatalogRefresh({
        phase: 'failed',
        reason: e instanceof Error ? e.message : String(e),
      })
    }
  }

  return (
    <div className="max-w-medium pr-xl pb-10">
      <p className="font-body-md text-on-surface-variant mb-md">{t('settings.models.subtitle')}</p>

      <div className="space-y-lg">
        {/* Performance Strategy */}
        <section className="bg-surface-container-lowest border border-outline-variant/30 rounded-xl p-lg shadow-e1">
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
                    ? 'bg-primary text-on-primary shadow-e1 ring-1 ring-black/5 font-bold'
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
        <section className="bg-surface-container-lowest border border-outline-variant/30 rounded-xl p-lg shadow-e1">
          <h3 className="font-headline-md text-on-surface mb-md">{t('settings.models.activeModel')}</h3>
          {currentModel ? (
            <div className="p-md rounded-xl border-2 border-primary bg-primary-container/5 flex items-center justify-between transition-all">
              <div className="flex items-center gap-md">
                <div className="w-10 h-10 rounded-lg bg-primary text-on-primary flex items-center justify-center">
                  <span className="material-symbols-outlined">auto_awesome</span>
                </div>
                <div>
                  <div className="flex items-center gap-xs">
                    <span className="font-headline-md text-link text-lg">{currentModel}</span>
                    <span className="px-xs py-[2px] bg-primary text-on-primary rounded-sm text-label-2xs font-bold">{t('settings.models.activeBadge')}</span>
                  </div>
                  <p className="text-label-sm text-on-surface-variant">{intl.formatMessage({ id: 'settings.models.providerLabel' }, { provider: status?.provider })}</p>
                </div>
              </div>
            </div>
          ) : (
            <p className="text-body-sm text-on-surface-variant">{t('settings.models.noModelSelected')}</p>
          )}

          {/* Quick switcher (Phase B): searchable ComboboxSelect over the full
              catalog — the provider sections below stay as the detailed view. */}
          <div className="mt-md max-w-md">
            <ComboboxSelect
              label={t('settings.models.quickSwitch.label')}
              placeholder={t('settings.models.quickSwitch.placeholder')}
              emptyText={t('settings.models.quickSwitch.empty')}
              options={models.map(m => ({ value: m.id, label: `${m.name} · ${m.provider}` }))}
              value={currentModel ?? null}
              onChange={(v) => { if (v) void handleModelSwitch(v) }}
              disabled={switching != null}
            />
          </div>
        </section>

        {/* R3-3: plan/act model tiers (global preference; the chat header's
            compact pair writes the same config keys). */}
        <PhaseTierSection />

        {/* R3-2: named provider model profiles (list / switch / create —
            rename & delete deferred). A switch refreshes provider status +
            catalog through the same paths provider activation uses. */}
        <ProfilesSection onSwitched={async () => { await Promise.all([refreshModels(), refreshStatus()]) }} />

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
        <section className="bg-surface-container-lowest border border-outline-variant/30 rounded-xl shadow-e1 overflow-hidden">
          <div className="border-b border-outline-variant/30 bg-surface-container-low/30 px-lg pt-md">
            <div className="flex gap-lg overflow-x-auto">
              <Button
                variant="ghost"
                onClick={() => setActiveProvider(null)}
                className={cn(
                  'h-auto pb-sm px-xs border-b-2 font-label-md whitespace-nowrap cursor-pointer transition-colors rounded-none',
                  !activeProvider ? 'border-primary text-link font-bold' : 'border-transparent text-on-surface-variant hover:text-primary',
                )}
              >{t('settings.models.tabAll')}</Button>
              {providers.map(p => (
                <Button
                  key={p}
                  variant="ghost"
                  onClick={() => setActiveProvider(activeProvider === p ? null : p)}
                  className={cn(
                    'h-auto pb-sm px-xs border-b-2 font-label-md whitespace-nowrap cursor-pointer transition-colors rounded-none',
                    activeProvider === p ? 'border-primary text-link font-bold' : 'border-transparent text-on-surface-variant hover:text-primary',
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
              <div className="flex items-center gap-sm shrink-0">
                {/* R2-2: manual dynamic-catalog refresh (models.dev overlay) —
                    previously reachable only via the CLI `/model refresh`. */}
                <Button
                  variant="outline"
                  onClick={() => { void handleRefreshCatalog() }}
                  disabled={catalogRefresh.phase === 'busy'}
                  aria-label={t('settings.models.refreshCatalog.aria')}
                  title={t('settings.models.refreshCatalog.aria')}
                  data-testid="refresh-model-catalog"
                  className="h-auto py-sm px-md rounded-lg font-label-md flex items-center gap-xs cursor-pointer"
                >
                  {catalogRefresh.phase === 'busy' ? (
                    <Spinner className="text-primary icon-sm" />
                  ) : (
                    <span className="material-symbols-outlined icon-sm" aria-hidden="true">refresh</span>
                  )}
                  {t('settings.models.refreshCatalog')}
                </Button>
                <span className="inline-flex items-center px-sm py-xs bg-primary-container text-on-primary-container rounded-full text-label-2xs font-bold tracking-wider uppercase">
                  {intl.formatMessage({ id: 'settings.models.count' }, { count: models.length })}
                </span>
              </div>
            </div>

            {/* R2-2: refresh outcome — success count or the upstream failure
                reason inline (never a silent no-op). */}
            {catalogRefresh.phase === 'done' && (
              <p role="status" data-testid="refresh-model-catalog-result" className="mb-md text-label-sm text-on-surface-variant flex items-center gap-xs">
                <span className="material-symbols-outlined icon-sm text-success" aria-hidden="true">check_circle</span>
                {intl.formatMessage({ id: 'settings.models.refreshCatalog.success' }, { count: catalogRefresh.count })}
              </p>
            )}
            {catalogRefresh.phase === 'failed' && (
              <p role="alert" data-testid="refresh-model-catalog-result" className="mb-md text-label-sm text-error flex items-center gap-xs">
                <span className="material-symbols-outlined icon-sm" aria-hidden="true">error</span>
                {intl.formatMessage({ id: 'settings.models.refreshCatalog.failed' }, { reason: catalogRefresh.reason })}
              </p>
            )}

            {filteredModels.length === 0 ? (
              <p className="text-body-sm text-on-surface-variant py-lg text-center">{t('settings.models.noModelsFound')}</p>
            ) : (
              <div className="grid grid-cols-1 gap-md">
                {filteredModels.map(m => {
                  const inVault = isInVault(vault?.models, m.id)
                  const expanded = expandedMeta === m.id
                  return (
                <div
                  key={m.id}
                  data-testid="catalog-model-row"
                  className={cn(
                    'rounded-xl border transition-all overflow-hidden',
                    m.id === currentModel ? 'border-2 border-primary bg-primary-container/5' : 'border-outline-variant/50 hover:border-primary/50',
                  )}
                >
                  <div className="flex items-stretch">
                    <Button
                      variant="ghost"
                      onClick={() => handleModelSwitch(m.id)}
                      disabled={switching !== null}
                      className="h-auto p-md rounded-xl flex-1 min-w-0 flex items-center justify-between hover:bg-transparent group cursor-pointer text-left w-full whitespace-normal"
                    >
                      <div className="flex items-center gap-md">
                        <div className={cn("w-10 h-10 rounded-lg flex items-center justify-center shrink-0",
                          m.id === currentModel ? 'bg-primary text-on-primary' : 'bg-surface-container-high text-on-surface-variant',
                        )}>
                          <span className="material-symbols-outlined">psychology</span>
                        </div>
                        <div>
                          <div className="flex items-center gap-xs">
                            <span className={cn("font-headline-md text-lg", m.id === currentModel ? 'text-link' : 'text-on-surface')}>{m.name}</span>
                            {m.id === currentModel ? <span className="px-xs py-[2px] bg-primary text-on-primary rounded-sm text-label-2xs font-bold">{t('settings.models.defaultBadge')}</span> : null}
                            {m.tier ? (
                              <span
                                className="px-xs py-[2px] bg-primary text-on-primary rounded-sm text-label-2xs font-bold uppercase tracking-wider"
                                title={t('settings.models.tier')}
                              >
                                {t(`settings.models.tier${m.tier.charAt(0).toUpperCase()}${m.tier.slice(1)}` as 'settings.models.tierFast' | 'settings.models.tierStandard' | 'settings.models.tierPro')}
                              </span>
                            ) : null}
                            {/* S2-1 (裁定③/S2-1 source badge): honest provenance —
                                `overlay` rows come from the models.dev refresh,
                                `declared` rows from the provider's curated vault
                                (AddProviderModal fetch 固化). Catalog rows are the
                                default and stay unbadged. (The old always-off
                                `dynamic` badge was removed in S1-3; `source` is
                                its replacement.) S3-1: rendered through the
                                SHARED badge component so the composer chip and
                                the Header picker wear the identical styling. */}
                            <ModelSourceBadge source={m.source} />
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
                            {' · '}
                            {/* S2-3: declared/catalog max output per request —
                                unknown renders "—" (honest metadata). */}
                            {intl.formatMessage(
                              { id: 'settings.models.maxOutput' },
                              {
                                value:
                                  m.max_output != null && m.max_output > 0
                                    ? m.max_output >= 1000
                                      ? `${(m.max_output / 1000).toFixed(0)}k`
                                      : String(m.max_output)
                                    : '—',
                              },
                            )}
                          </p>
                        </div>
                      </div>
                      {switching === m.id ? (
                        <Spinner className="text-primary text-headline-sm" />
                      ) : null}
                    </Button>
                    {/* S2-2 vault affordance column. Rows already declared get
                        the expand-to-edit toggle; catalog/overlay rows get the
                        "add to vault" action (persists an id-only declaration,
                        then opens the editor for metadata). Hidden entirely
                        when no managed provider slot is active. */}
                    {vault ? (
                      inVault ? (
                        <Button
                          variant="ghost"
                          data-testid={`edit-model-meta-${m.id}`}
                          aria-label={intl.formatMessage({ id: 'settings.models.vault.editAria' }, { model: m.name })}
                          aria-expanded={expanded}
                          onClick={() => setExpandedMeta(expanded ? null : m.id)}
                          className="shrink-0 self-center mr-sm px-sm py-xs rounded-lg text-on-surface-variant hover:text-primary hover:bg-surface-container-high cursor-pointer"
                        >
                          <span className="material-symbols-outlined icon-sm" aria-hidden="true">tune</span>
                        </Button>
                      ) : (
                        <Button
                          variant="ghost"
                          data-testid={`add-model-to-vault-${m.id}`}
                          aria-label={intl.formatMessage({ id: 'settings.models.vault.addToVaultAria' }, { model: m.name })}
                          disabled={addingToVault !== null}
                          onClick={() => { void handleAddToVault(m.id) }}
                          className="shrink-0 self-center mr-sm px-sm py-xs rounded-lg text-on-surface-variant hover:text-primary hover:bg-surface-container-high cursor-pointer disabled:opacity-50"
                        >
                          {addingToVault === m.id ? (
                            <Spinner className="text-primary icon-sm" />
                          ) : (
                            <span className="material-symbols-outlined icon-sm" aria-hidden="true">add_circle</span>
                          )}
                        </Button>
                      )
                    ) : null}
                  </div>
                  {vault && expanded ? (
                    <ModelMetaEditor
                      modelId={m.id}
                      spec={vault.models.find(s => s.id === m.id)}
                      saving={savingMeta}
                      onSave={(input) => { void handleSaveMeta(input) }}
                      onCancel={() => setExpandedMeta(null)}
                    />
                  ) : null}
                </div>
                  )
                })}
              </div>
            )}
          </div>
        </section>

        {/* Global Parameters */}
        <section className="bg-surface-container-lowest border border-outline-variant/30 rounded-xl p-lg shadow-e1">
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