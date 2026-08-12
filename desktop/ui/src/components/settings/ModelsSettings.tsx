import { useEffect, useState } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import AddProviderModal from '@/components/settings/AddProviderModal'
import { useCatalog } from '@/context/CatalogContext'
import * as api from '@/lib/tauri-api'
import { toastError } from '@/lib/errorToast'
import type {
  ProviderConnection,
  ProvidersFile,
} from '@/types'

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
              <button
                key={s}
                onClick={() => setStrategy(s)}
                className={`flex-1 py-sm font-label-md rounded-lg transition-all cursor-pointer ${
                  strategy === s
                    ? 'bg-surface-container-lowest text-primary shadow-sm ring-1 ring-black/5 font-bold'
                    : 'text-on-surface-variant hover:bg-surface-container-high'
                }`}
              >
                {s === 'high-quality' ? t('settings.models.stratLabel.highQuality') : s === 'speed' ? t('settings.models.stratLabel.speed') : t('settings.models.stratLabel.balanced')}
              </button>
            ))}
          </div>
          <p className="mt-md text-label-sm text-on-surface-variant opacity-70 flex items-center gap-xs">
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
                  <p className="text-label-sm text-on-surface-variant opacity-70">{intl.formatMessage({ id: 'settings.models.providerLabel' }, { provider: status?.provider })}</p>
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
                          title="From models.dev (live)"
                        >
                          {t('settings.models.dynamicBadge')}
                        </span>
                      ) : null}
                    </div>
                    <p className="text-label-sm text-on-surface-variant opacity-70">
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
                  </button>
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

// ===== Managed providers (Models P2) =====

interface KindInfo {
  labelKey: string
  icon: string
  baseUrlRequired: boolean
  needsKey: boolean
}

const KIND_INFO: Record<string, KindInfo> = {
  anthropic: { labelKey: 'settings.models.providers.kinds.anthropic', icon: 'auto_awesome', baseUrlRequired: false, needsKey: true },
  openai: { labelKey: 'settings.models.providers.kinds.openai', icon: 'bolt', baseUrlRequired: false, needsKey: true },
  deepseek: { labelKey: 'settings.models.providers.kinds.deepseek', icon: 'psychology', baseUrlRequired: false, needsKey: true },
  ollama: { labelKey: 'settings.models.providers.kinds.ollama', icon: 'dns', baseUrlRequired: false, needsKey: false },
  gemini: { labelKey: 'settings.models.providers.kinds.gemini', icon: 'spark', baseUrlRequired: false, needsKey: true },
  'openai-compatible': { labelKey: 'settings.models.providers.kinds.openaiCompatible', icon: 'hub', baseUrlRequired: true, needsKey: true },
}

function kindLabel(intl: ReturnType<typeof useIntl>, kind: string): string {
  return intl.formatMessage({ id: KIND_INFO[kind]?.labelKey ?? 'settings.models.providers.kinds.openaiCompatible' })
}

/**
 * Format a per-million-token USD price for the model list. Returns the
 * i18n "unknown" placeholder for null / non-finite values so the UI
 * never invents a number (ADR-0005 P0-2 honest-cost). Two decimals
 * are enough resolution for a sidebar; the engine pricing SSOT is the
 * canonical source.
 */
function formatPrice(value: number | null | undefined): string {
  if (value == null || !Number.isFinite(value)) {
    return '—'
  }
  return value.toFixed(2)
}

function toastTestResult(
  intl: ReturnType<typeof useIntl>,
  result: api.TestConnectionResult,
  provider: string,
): void {
  const t = (id: string) => intl.formatMessage({ id })
  switch (result.kind) {
    case 'success':
      toast.success(t('settings.models.testResult.success'))
      return
    case 'invalid_key':
      toast.error(t('settings.models.testResult.invalidKey'))
      return
    case 'rate_limited':
      toast.warning(t('settings.models.testResult.rateLimited'))
      return
    case 'provider_error':
      toast.error(intl.formatMessage({ id: 'settings.models.testResult.providerError' }, { provider, status: result.status }))
      return
    case 'network_unreachable':
      toast.error(intl.formatMessage({ id: 'settings.models.testResult.networkUnreachable' }, { provider }))
      return
    case 'unknown':
      toast.error(intl.formatMessage({ id: 'settings.models.testResult.unknown' }, { message: result.message }))
      return
  }
}

/**
 * Renders the results of the "Test all providers" fan-out probe as a compact
 * list of provider label + status pill + latency. Status pill reuses the
 * same `settings.models.testResult.*` keys the single-provider toast uses so
 * the wording stays identical between the two surfaces.
 */
function TestAllResultsPanel({
  rows,
  intl,
  t,
}: {
  rows: api.ProviderTestRow[]
  intl: ReturnType<typeof useIntl>
  t: (id: string) => string
}) {
  const okCount = rows.filter(r => r.result.kind === 'success').length
  const summary = intl.formatMessage(
    { id: 'settings.models.providers.testAllSummary' },
    { ok: okCount, total: rows.length },
  )
  return (
    <div
      data-testid="test-all-results"
      className="mt-md rounded-lg border border-outline-variant/30 bg-surface-container-low/30 p-md space-y-sm"
    >
      <p className="font-label-sm text-on-surface-variant">{summary}</p>
      <div className="grid grid-cols-1 gap-xs">
        {rows.map(r => {
          const result = r.result
          const pillClass =
            result.kind === 'success'
              ? 'bg-primary-container text-on-primary-container'
              : result.kind === 'rate_limited'
                ? 'bg-tertiary-container text-on-tertiary-container'
                : 'bg-error-container text-on-error-container'
          const pillLabel = (() => {
            switch (result.kind) {
              case 'success':
                return t('settings.models.testResult.success')
              case 'invalid_key':
                return t('settings.models.testResult.invalidKey')
              case 'rate_limited':
                return t('settings.models.testResult.rateLimited')
              case 'network_unreachable':
                return intl.formatMessage({ id: 'settings.models.testResult.networkUnreachable' }, { provider: r.provider_kind })
              case 'provider_error':
                return intl.formatMessage({ id: 'settings.models.testResult.providerError' }, { provider: r.provider_kind, status: result.status })
              case 'unknown':
                return intl.formatMessage({ id: 'settings.models.testResult.unknown' }, { message: result.message })
            }
          })()
          return (
            <div
              key={r.id}
              className="flex items-center justify-between gap-md px-sm py-xs rounded-md bg-surface-container-lowest"
            >
              <div className="flex items-center gap-sm min-w-0">
                <span className="font-label-md text-on-surface truncate">{r.label}</span>
                <span className="font-label-xs text-[11px] text-on-surface-variant">{r.provider_kind}</span>
              </div>
              <div className="flex items-center gap-sm shrink-0">
                {r.latency_ms !== null ? (
                  <span className="font-label-xs text-[11px] text-on-surface-variant">
                    {intl.formatMessage({ id: 'settings.models.providers.latency' }, { ms: r.latency_ms })}
                  </span>
                ) : null}
                <span
                  data-testid={`test-all-result-${r.id}`}
                  className={`px-sm py-[2px] rounded-full text-[10px] font-bold uppercase tracking-wider ${pillClass}`}
                  title={pillLabel}
                >
                  {pillLabel}
                </span>
              </div>
            </div>
          )
        })}
      </div>
    </div>
  )
}

function ProvidersSection({
  providersFile,
  loading,
  onChange,
  onActivated,
}: {
  providersFile: ProvidersFile
  loading: boolean
  onChange: (f: ProvidersFile) => void
  onActivated: () => Promise<void>
}) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const [modalOpen, setModalOpen] = useState(false)
  const [editing, setEditing] = useState<ProviderConnection | null>(null)
  const [testingId, setTestingId] = useState<string | null>(null)
  const [activatingId, setActivatingId] = useState<string | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<ProviderConnection | null>(null)
  const [testAllRunning, setTestAllRunning] = useState(false)
  const [testAllRows, setTestAllRows] = useState<api.ProviderTestRow[] | null>(null)

  const handleTest = async (conn: ProviderConnection) => {
    // Only the active provider's key is mirrored into config; for a connection
    // we can only test when a key is set on it. Ollama needs no key.
    const info = KIND_INFO[conn.kind]
    if (info?.needsKey && !conn.has_api_key) {
      toast.error(intl.formatMessage({ id: 'settings.models.providers.needKey' }, { label: conn.display_name }))
      return
    }
    setTestingId(conn.id)
    try {
      // TD-4: the wire type no longer carries the raw api_key — only
      // `has_api_key: bool`. The backend reads the key from the credential
      // store; pass an empty string so the prompt-for-key flow triggers
      // when no key is resolvable.
      const apiKey = ''
      if (info?.needsKey && !conn.has_api_key) {
        toast.error(intl.formatMessage({ id: 'settings.models.providers.reenterKey' }, { label: conn.display_name }))
        return
      }
      const result = await api.testProviderConnection(conn.kind, apiKey, conn.base_url ?? undefined)
      toastTestResult(intl, result, conn.kind)
    } catch (e) {
      toastError(t('settings.models.testResult.failed'), e)
    } finally {
      setTestingId(null)
    }
  }

  const handleActivate = async (conn: ProviderConnection) => {
    setActivatingId(conn.id)
    try {
      await api.setActiveProvider(conn.id)
      // Re-fetch to pick up the masked file the backend persisted.
      const fresh = await api.listProviders()
      onChange(fresh)
      await onActivated()
      toast.success(intl.formatMessage({ id: 'settings.models.providers.activated' }, { label: conn.display_name }))
    } catch (e) {
      toastError(t('settings.models.providers.activateFailed'), e)
    } finally {
      setActivatingId(null)
    }
  }

  // Delete confirmation flows through the ConfirmDialog (state-driven) instead
  // of a native window.confirm, so it matches the app's design system and locale.
  const confirmDeleteProvider = async () => {
    const conn = deleteTarget
    setDeleteTarget(null)
    if (!conn) return
    try {
      const fresh = await api.deleteProvider(conn.id)
      onChange(fresh)
      toast.success(intl.formatMessage({ id: 'settings.models.providers.deleted' }, { label: conn.display_name }))
    } catch (e) {
      toastError(t('settings.models.providers.deleteFailed'), e)
    }
  }

  const handleSaved = (fresh: ProvidersFile) => {
    onChange(fresh)
    setModalOpen(false)
    setEditing(null)
    // Stale "test all" results would mislead if the user then re-runs the
    // batch probe — drop them and force the user to re-run.
    setTestAllRows(null)
    toast.success(t('settings.models.providers.saved'))
  }

  const handleTestAll = async () => {
    if (testAllRunning) return
    setTestAllRunning(true)
    try {
      const rows = await api.testAllProviders()
      setTestAllRows(rows)
    } catch (e) {
      toastError(t('settings.models.testResult.failed'), e)
    } finally {
      setTestAllRunning(false)
    }
  }

  return (
    <section className="bg-surface-container-lowest border border-outline-variant/30 rounded-xl p-lg shadow-sm">
      <div className="flex items-center justify-between mb-md">
        <div>
          <h3 className="font-headline-md text-on-surface">{t('settings.models.providers.title')}</h3>
          <p className="text-body-sm text-on-surface-variant">{t('settings.models.providers.subtitle')}</p>
        </div>
        <Button
          className="px-md py-sm bg-primary text-on-primary font-label-md rounded-lg hover:bg-primary/90 transition-colors flex items-center gap-sm whitespace-nowrap cursor-pointer"
          onClick={() => { setEditing(null); setModalOpen(true) }}
        >
          <span className="material-symbols-outlined text-[18px]">add</span>
          {t('settings.models.providers.add')}
        </Button>
      </div>
      <div className="flex justify-end -mt-md mb-md">
        <Button
          variant="ghost"
          className="px-md py-sm text-on-surface-variant hover:text-primary whitespace-nowrap cursor-pointer disabled:opacity-50"
          onClick={handleTestAll}
          disabled={testAllRunning || providersFile.providers.length === 0}
          aria-label={t('settings.models.providers.testAll')}
        >
          {testAllRunning ? (
            <>
              <span className="material-symbols-outlined text-[18px] animate-spin">progress_activity</span>
              {t('settings.models.providers.testAllInProgress')}
            </>
          ) : (
            <>
              <span className="material-symbols-outlined text-[18px]">cable</span>
              {t('settings.models.providers.testAll')}
            </>
          )}
        </Button>
      </div>

      {loading ? (
        <p className="text-body-sm text-on-surface-variant py-lg text-center">{t('settings.models.providers.loading')}</p>
      ) : providersFile.providers.length === 0 ? (
        <p className="text-body-sm text-on-surface-variant py-lg text-center">{t('settings.models.providers.empty')}</p>
      ) : (
        <div className="grid grid-cols-1 gap-sm">
          {providersFile.providers.map(conn => {
            const isActive = providersFile.active_provider_id === conn.id
            const hasKey = conn.has_api_key
            const info = KIND_INFO[conn.kind]
            return (
              <div
                key={conn.id}
                className={`p-md rounded-xl border flex items-center justify-between transition-colors ${
                  isActive ? 'border-2 border-primary bg-primary-container/5' : 'border-outline-variant/50'
                }`}
              >
                <div className="flex items-center gap-md min-w-0">
                  <div className={`w-9 h-9 rounded-lg flex items-center justify-center shrink-0 ${isActive ? 'bg-primary text-on-primary' : 'bg-surface-container-high text-on-surface-variant'}`}>
                    <span className="material-symbols-outlined icon-md">{info?.icon ?? 'hub'}</span>
                  </div>
                  <div className="min-w-0">
                    <div className="flex items-center gap-xs">
                      <span className="font-label-md font-bold text-on-surface truncate">{conn.display_name}</span>
                      {isActive ? (
                        <span className="px-xs py-[1px] bg-primary text-on-primary rounded text-[10px] font-bold shrink-0">{t('settings.models.providers.activeBadge')}</span>
                      ) : null}
                    </div>
                    <div className="flex items-center gap-xs flex-wrap">
                      <span className="font-label-xs text-[11px] text-on-surface-variant">{kindLabel(intl, conn.kind)}</span>
                      {conn.base_url ? (
                        <span className="font-label-xs text-[11px] text-on-surface-variant font-mono truncate max-w-[260px]" title={conn.base_url ?? undefined}>{conn.base_url}</span>
                      ) : null}
                      <span
                        className={`inline-flex items-center gap-[2px] font-label-xs text-[10px] ${hasKey ? 'text-primary' : 'text-on-surface-variant opacity-60'}`}
                        title={hasKey ? t('settings.models.providers.keySet') : t('settings.models.providers.keyMissing')}
                      >
                        <span className="material-symbols-outlined text-[12px]">{hasKey ? 'key' : 'key_off'}</span>
                        {hasKey ? t('settings.models.providers.keySet') : t('settings.models.providers.keyMissing')}
                      </span>
                    </div>
                  </div>
                </div>
                <div className="flex items-center gap-xs shrink-0">
                  {testingId === conn.id ? (
                    <span className="material-symbols-outlined text-primary animate-spin text-[18px]">progress_activity</span>
                  ) : (
                    <Button variant="ghost" className="px-sm py-xs text-on-surface-variant hover:text-primary cursor-pointer" onClick={() => handleTest(conn)} aria-label={t('settings.models.providers.test')}>
                      <span className="material-symbols-outlined text-[18px]">cable</span>
                    </Button>
                  )}
                  {!isActive ? (
                    <Button
                      variant="ghost"
                      className="px-sm py-xs text-primary hover:bg-primary/10 cursor-pointer disabled:opacity-50"
                      onClick={() => handleActivate(conn)}
                      disabled={activatingId !== null}
                    >
                      {activatingId === conn.id ? (
                        <span className="material-symbols-outlined text-[16px] animate-spin align-middle">progress_activity</span>
                      ) : t('settings.models.providers.activate')}
                    </Button>
                  ) : null}
                  <Button variant="ghost" className="px-sm py-xs text-on-surface-variant hover:text-primary cursor-pointer" onClick={() => { setEditing(conn); setModalOpen(true) }} aria-label={t('settings.models.providers.edit')}>
                    <span className="material-symbols-outlined text-[18px]">edit</span>
                  </Button>
                  <Button variant="ghost" className="px-sm py-xs text-on-surface-variant hover:text-error cursor-pointer" onClick={() => setDeleteTarget(conn)} aria-label={t('settings.models.providers.delete')}>
                    <span className="material-symbols-outlined text-[18px]">delete</span>
                  </Button>
                </div>
              </div>
            )
          })}
        </div>
      )}

      {testAllRows !== null ? (
        testAllRows.length === 0 ? (
          <p className="text-body-sm text-on-surface-variant py-md text-center">{t('settings.models.providers.testAllEmpty')}</p>
        ) : (
          <TestAllResultsPanel rows={testAllRows} intl={intl} t={t} />
        )
      ) : null}

      {modalOpen ? (
        <AddProviderModal
          editing={editing}
          onClose={() => { setModalOpen(false); setEditing(null) }}
          onSaved={handleSaved}
        />
      ) : null}

      {deleteTarget ? (
        <ConfirmDialog
          open
          destructive
          title={t('settings.models.providers.deleteConfirmTitle')}
          message={intl.formatMessage({ id: 'settings.models.providers.confirmDelete' }, { label: deleteTarget.display_name })}
          confirmLabel={t('settings.models.providers.delete')}
          cancelLabel={t('settings.models.providers.cancel')}
          onConfirm={confirmDeleteProvider}
          onCancel={() => setDeleteTarget(null)}
        />
      ) : null}
    </section>
  )
}

// === Provider visibility (ADR-0005 P4.9) ===
//
// Settings panel that toggles which provider kinds appear in the model
// picker. Backed by `DesktopConfig.enabled_providers` (persisted to
// `~/.shannon/desktop/config.json`); the engine's `SHANNON_*_PROVIDERS`
// env vars are honoured only when the override is `null`.
//
// `null` (no override) is rendered as every checkbox checked — the user
// sees the engine env-var state would apply, even though no explicit
// state is persisted. `[]` is rendered as none checked. Otherwise only
// the listed slugs are checked.

const PROVIDER_KINDS_FOR_VISIBILITY = [
  'anthropic',
  'openai',
  'deepseek',
  'ollama',
  'gemini',
  'openai-compatible',
] as const

function ProviderVisibilitySection({
  onChanged,
}: {
  onChanged: () => Promise<void>
}) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  // `null` ⇒ no desktop override (engine env vars decide).
  // `Some([])` ⇒ user toggled every provider off.
  // `Some([..])` ⇒ explicit allowlist.
  const [override, setOverride] = useState<string[] | null>(null)
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)

  useEffect(() => {
    let cancelled = false
    api.getProviderAllowlist()
      .then(() => {
        if (!cancelled) {
          // The backend returns the *effective* allowlist (env vars
          // resolved when desktop override is `null`). For the UI we
          // want the desktop override specifically — `null` ⇒
          // "use env vars", `Some([])` ⇒ "all off",
          // `Some(non_empty)` ⇒ explicit list. Read `enabled_providers`
          // through `getConfig` to disambiguate.
          api.getConfig().then((cfg) => {
            if (cancelled) return
            const ep = (cfg as { enabled_providers?: string[] | null }).enabled_providers
            setOverride(ep === undefined ? null : ep)
            setLoading(false)
          }).catch(() => { if (!cancelled) setLoading(false) })
        }
      })
      .catch(() => { if (!cancelled) setLoading(false) })
    return () => { cancelled = true }
  }, [])

  const isChecked = (kind: string): boolean => {
    // `null` (no override) ⇒ all checked (engine env-var decides).
    // `Some([])` ⇒ none checked. `Some(non_empty)` ⇒ only those.
    if (override === null) return true
    return override.includes(kind)
  }

  const toggle = async (kind: string) => {
    if (saving) return
    const current = override === null
      ? [...PROVIDER_KINDS_FOR_VISIBILITY]
      : override
    const next = current.includes(kind)
      ? current.filter((k) => k !== kind)
      : [...current, kind]
    setSaving(true)
    try {
      await api.configure({
        key: 'enabled_providers',
        value: JSON.stringify(next),
      })
      setOverride(next)
      await onChanged()
    } catch (e) {
      toastError(t('settings.models.providers.saveFailed'), e)
    } finally {
      setSaving(false)
    }
  }

  const resetToDefault = async () => {
    if (saving) return
    setSaving(true)
    try {
      // `null` clears the desktop override (falls back to env vars).
      await api.configure({ key: 'enabled_providers', value: 'null' })
      setOverride(null)
      await onChanged()
      toast.success(t('settings.models.providerVisibility.reset'))
    } catch (e) {
      toastError(t('settings.models.providers.saveFailed'), e)
    } finally {
      setSaving(false)
    }
  }

  return (
    <section className="bg-surface-container-lowest border border-outline-variant/30 rounded-xl p-lg shadow-sm">
      <div className="flex items-start justify-between mb-md gap-md">
        <div>
          <h3 className="font-headline-md text-on-surface">
            {t('settings.models.providerVisibility.title')}
          </h3>
          <p className="text-body-sm text-on-surface-variant mt-xs">
            {t('settings.models.providerVisibility.subtitle')}
          </p>
        </div>
        <Button
          variant="ghost"
          className="px-md py-sm text-on-surface-variant hover:text-primary whitespace-nowrap cursor-pointer disabled:opacity-50"
          onClick={resetToDefault}
          disabled={loading || saving || override === null}
        >
          {t('settings.models.providerVisibility.reset')}
        </Button>
      </div>

      <div className="grid grid-cols-1 sm:grid-cols-2 gap-sm">
        {PROVIDER_KINDS_FOR_VISIBILITY.map((kind) => {
          const checked = isChecked(kind)
          return (
            <label
              key={kind}
              className={`flex items-center gap-md p-sm rounded-lg border cursor-pointer transition-colors ${
                checked
                  ? 'border-primary/50 bg-primary-container/5'
                  : 'border-outline-variant/30 hover:border-outline-variant'
              }`}
            >
              <input
                type="checkbox"
                className="w-4 h-4 cursor-pointer accent-primary"
                checked={checked}
                disabled={loading || saving}
                onChange={() => toggle(kind)}
              />
              <span className="font-label-md text-on-surface">
                {kindLabel(intl, kind)}
              </span>
            </label>
          )
        })}
      </div>
    </section>
  )
}

function ParameterSlider({ label, value, min, max, step, formatValue, lowLabel, highLabel, configKey }: {
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
  const [local, setLocal] = useState(value)
  // Keep the slider in sync with the persisted config value so it reflects
  // reality (initial load + external updates) instead of a stale literal.
  useEffect(() => { setLocal(value) }, [value])
  const display = formatValue ? formatValue(local) : String(local)

  const handleChange = (newValue: number) => {
    setLocal(newValue)
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
        min={min} max={max} step={step} type="range" value={local}
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
