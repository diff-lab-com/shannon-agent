import { useEffect, useState } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import { Input } from '@/components/ui/input'
import { useApp } from '@/context/AppContext'
import * as api from '@/lib/tauri-api'
import { toastError } from '@/lib/errorToast'
import type {
  ProviderConnection,
  ProviderInput,
  ProviderKind,
  ProvidersFile,
} from '@/types'

export default function ModelsSettings() {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const { models, status, config, refreshModels, refreshStatus } = useApp()
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
      await api.switchProvider({ provider: status.provider, model: modelId })
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
                        <p className="text-label-sm text-on-surface-variant opacity-70">{m.provider} {m.context_window > 0 ? intl.formatMessage({ id: 'settings.models.contextWindow' }, { count: (m.context_window / 1000).toFixed(0) }) : ''}</p>
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
  'openai-compatible': { labelKey: 'settings.models.providers.kinds.openaiCompatible', icon: 'hub', baseUrlRequired: true, needsKey: true },
}

interface QuickFill {
  id: string
  label: string
  icon: string
  kind: ProviderKind
  baseUrl?: string
  model?: string
}

// Quick-fill chips in the Add/Edit modal. The built-in providers map to their
// kind; GLM / Kimi / MiniMax are OpenAI-compatible endpoints (kind =
// `openai-compatible`), which the Rust layer tests with a Bearer token.
const QUICK_FILL: QuickFill[] = [
  { id: 'anthropic', label: 'Anthropic', icon: 'auto_awesome', kind: 'anthropic', model: 'claude-sonnet-4-6' },
  { id: 'openai', label: 'OpenAI', icon: 'bolt', kind: 'openai', model: 'gpt-4.1-mini' },
  { id: 'deepseek', label: 'DeepSeek', icon: 'psychology', kind: 'deepseek', model: 'deepseek-chat' },
  { id: 'glm', label: 'GLM (Zhipu)', icon: 'auto_awesome', kind: 'openai-compatible', baseUrl: 'https://open.bigmodel.cn/api/paas/v4', model: 'glm-4-plus' },
  { id: 'kimi', label: 'Kimi (Moonshot)', icon: 'dark_mode', kind: 'openai-compatible', baseUrl: 'https://api.moonshot.cn/v1', model: 'moonshot-v1-8k' },
  { id: 'minimax', label: 'MiniMax', icon: 'group', kind: 'openai-compatible', baseUrl: 'https://api.minimax.chat/v1', model: 'abab6.5s-chat' },
  { id: 'ollama', label: 'Ollama (local)', icon: 'dns', kind: 'ollama', baseUrl: 'http://localhost:11434', model: 'llama3.2' },
  { id: 'custom', label: 'settings.models.providers.customOpenAI', icon: 'hub', kind: 'openai-compatible' },
]

function kindLabel(intl: ReturnType<typeof useIntl>, kind: string): string {
  return intl.formatMessage({ id: KIND_INFO[kind]?.labelKey ?? 'settings.models.providers.kinds.openaiCompatible' })
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

  const handleTest = async (conn: ProviderConnection) => {
    // Only the active provider's key is mirrored into config; for a connection
    // we can only test when a key is set on it. Ollama needs no key.
    const info = KIND_INFO[conn.provider_kind]
    if (info?.needsKey && !conn.api_key) {
      toast.error(intl.formatMessage({ id: 'settings.models.providers.needKey' }, { label: conn.label }))
      return
    }
    setTestingId(conn.id)
    try {
      // Use the masked "***" sentinel only when it's the only thing available —
      // the backend can't test against a masked value, so for non-active
      // connections without a fresh key we ask the user to re-enter it.
      const apiKey = conn.api_key && conn.api_key !== '***' ? conn.api_key : ''
      if (info?.needsKey && !apiKey) {
        toast.error(intl.formatMessage({ id: 'settings.models.providers.reenterKey' }, { label: conn.label }))
        return
      }
      const result = await api.testProviderConnection(conn.provider_kind, apiKey, conn.base_url ?? undefined)
      toastTestResult(intl, result, conn.provider_kind)
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
      toast.success(intl.formatMessage({ id: 'settings.models.providers.activated' }, { label: conn.label }))
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
      toast.success(intl.formatMessage({ id: 'settings.models.providers.deleted' }, { label: conn.label }))
    } catch (e) {
      toastError(t('settings.models.providers.deleteFailed'), e)
    }
  }

  const handleSaved = (fresh: ProvidersFile) => {
    onChange(fresh)
    setModalOpen(false)
    setEditing(null)
    toast.success(t('settings.models.providers.saved'))
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

      {loading ? (
        <p className="text-body-sm text-on-surface-variant py-lg text-center">{t('settings.models.providers.loading')}</p>
      ) : providersFile.providers.length === 0 ? (
        <p className="text-body-sm text-on-surface-variant py-lg text-center">{t('settings.models.providers.empty')}</p>
      ) : (
        <div className="grid grid-cols-1 gap-sm">
          {providersFile.providers.map(conn => {
            const isActive = providersFile.active_provider_id === conn.id
            const hasKey = !!conn.api_key
            const info = KIND_INFO[conn.provider_kind]
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
                      <span className="font-label-md font-bold text-on-surface truncate">{conn.label}</span>
                      {isActive ? (
                        <span className="px-xs py-[1px] bg-primary text-on-primary rounded text-[10px] font-bold shrink-0">{t('settings.models.providers.activeBadge')}</span>
                      ) : null}
                    </div>
                    <div className="flex items-center gap-xs flex-wrap">
                      <span className="font-label-xs text-[11px] text-on-surface-variant">{kindLabel(intl, conn.provider_kind)}</span>
                      {conn.base_url ? (
                        <span className="font-label-xs text-[11px] text-on-surface-variant font-mono truncate max-w-[260px]" title={conn.base_url ?? undefined}>{conn.base_url}</span>
                      ) : null}
                      {conn.model ? (
                        <span className="font-label-xs text-[11px] text-on-surface-variant font-mono">{conn.model}</span>
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

      {modalOpen ? (
        <ProviderModal
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
          message={intl.formatMessage({ id: 'settings.models.providers.confirmDelete' }, { label: deleteTarget.label })}
          confirmLabel={t('settings.models.providers.delete')}
          cancelLabel={t('settings.models.providers.cancel')}
          onConfirm={confirmDeleteProvider}
          onCancel={() => setDeleteTarget(null)}
        />
      ) : null}
    </section>
  )
}

function ProviderModal({
  editing,
  onClose,
  onSaved,
}: {
  editing: ProviderConnection | null
  onClose: () => void
  onSaved: (f: ProvidersFile) => void
}) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const [label, setLabel] = useState(editing?.label ?? '')
  const [kind, setKind] = useState<string>(editing?.provider_kind ?? 'openai-compatible')
  const [baseUrl, setBaseUrl] = useState(editing?.base_url ?? '')
  const [apiKey, setApiKey] = useState('')
  const [model, setModel] = useState(editing?.model ?? '')
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const info = KIND_INFO[kind] ?? KIND_INFO['openai-compatible']

  const applyQuickFill = (qf: QuickFill) => {
    setKind(qf.kind)
    if (qf.baseUrl) setBaseUrl(qf.baseUrl)
    if (qf.model) setModel(qf.model)
    if (!label) setLabel(qf.id === 'custom' ? '' : qf.label)
  }

  const submit = async () => {
    const trimmedLabel = label.trim()
    if (!trimmedLabel) {
      setError(t('settings.models.providers.needLabel'))
      return
    }
    if (info.baseUrlRequired && !baseUrl.trim()) {
      setError(t('settings.models.providers.needBaseUrl'))
      return
    }
    setSaving(true)
    setError(null)
    const input: ProviderInput = {
      id: editing?.id,
      label: trimmedLabel,
      provider_kind: kind,
      // For a new connection require a key when the kind needs one; on edit,
      // an empty value tells the backend to keep the existing key.
      api_key: apiKey.trim() || undefined,
      base_url: baseUrl.trim() || undefined,
      model: model.trim() || undefined,
    }
    try {
      const fresh = await api.saveProvider(input)
      onSaved(fresh)
    } catch (e) {
      setError(String(e))
    } finally {
      setSaving(false)
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-md" onClick={onClose}>
      <div
        className="bg-surface-container-lowest border border-outline-variant/40 rounded-2xl shadow-xl w-full max-w-lg p-lg space-y-md"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between">
          <h3 className="font-headline-md text-on-surface">
            {editing ? t('settings.models.providers.editTitle') : t('settings.models.providers.addTitle')}
          </h3>
          <Button variant="ghost" className="text-on-surface-variant hover:text-primary cursor-pointer" onClick={onClose} aria-label={t('settings.models.providers.cancel')}>
            <span className="material-symbols-outlined">close</span>
          </Button>
        </div>

        {/* Quick fill */}
        <div>
          <p className="font-label-sm text-on-surface-variant mb-xs">{t('settings.models.providers.quickFill')}</p>
          <div className="flex flex-wrap gap-xs">
            {QUICK_FILL.map(qf => (
              <button
                key={qf.id}
                type="button"
                onClick={() => applyQuickFill(qf)}
                className="inline-flex items-center gap-xs px-sm py-xs rounded-lg border border-outline-variant/40 bg-surface-container-low/40 hover:border-primary/40 hover:bg-primary/5 text-on-surface-variant hover:text-primary font-label-sm text-[12px] cursor-pointer"
              >
                <span className="material-symbols-outlined text-[14px]">{qf.icon}</span>
                {qf.id === 'custom' ? t(qf.label) : qf.label}
              </button>
            ))}
          </div>
        </div>

        <div className="space-y-sm">
          <Field label={t('settings.models.providers.labelField')}>
            <Input className="w-full px-md py-sm bg-surface text-on-surface border border-outline-variant/50 rounded-lg outline-none focus:ring-2 focus:ring-primary font-body-sm" value={label} onChange={(e) => { setLabel(e.target.value); setError(null) }} placeholder={t('settings.models.providers.labelPlaceholder')} autoFocus />
          </Field>

          <Field label={t('settings.models.providers.kindField')}>
            <select
              className="w-full px-md py-sm bg-surface text-on-surface border border-outline-variant/50 rounded-lg outline-none focus:ring-2 focus:ring-primary font-body-sm cursor-pointer"
              value={kind}
              onChange={(e) => setKind(e.target.value)}
            >
              {Object.keys(KIND_INFO).map(k => (
                <option key={k} value={k}>{kindLabel(intl, k)}</option>
              ))}
            </select>
          </Field>

          <Field label={t(info.baseUrlRequired ? 'settings.models.providers.baseUrlRequired' : 'settings.models.providers.baseUrlOptional')}>
            <Input className="w-full px-md py-sm bg-surface text-on-surface border border-outline-variant/50 rounded-lg outline-none focus:ring-2 focus:ring-primary font-body-sm font-mono" value={baseUrl} onChange={(e) => { setBaseUrl(e.target.value); setError(null) }} placeholder="https://api.example.com/v1" />
          </Field>

          <Field label={t('settings.models.providers.apiKeyField')}>
            <Input
              className="w-full px-md py-sm bg-surface text-on-surface border border-outline-variant/50 rounded-lg outline-none focus:ring-2 focus:ring-primary font-body-sm font-mono"
              type="password"
              value={apiKey}
              onChange={(e) => { setApiKey(e.target.value); setError(null) }}
              placeholder={editing ? t('settings.models.providers.apiKeyKeep') : t('settings.models.providers.apiKeyPlaceholder')}
              disabled={!info.needsKey}
            />
          </Field>

          <Field label={t('settings.models.providers.modelField')}>
            <Input className="w-full px-md py-sm bg-surface text-on-surface border border-outline-variant/50 rounded-lg outline-none focus:ring-2 focus:ring-primary font-body-sm font-mono" value={model} onChange={(e) => setModel(e.target.value)} placeholder="claude-sonnet-4-6" />
          </Field>
        </div>

        {error ? (
          <div className="font-label-sm text-[12px] text-error">{error}</div>
        ) : null}

        <div className="flex justify-end gap-sm pt-xs">
          <Button className="px-md py-sm border border-outline-variant bg-surface-container-lowest text-on-surface font-label-md rounded-lg hover:bg-surface-container cursor-pointer" onClick={onClose}>
            {t('settings.models.providers.cancel')}
          </Button>
          <Button className="px-lg py-sm bg-primary text-on-primary font-label-md rounded-lg hover:bg-primary/90 transition-colors flex items-center gap-sm cursor-pointer disabled:opacity-50" onClick={submit} disabled={saving}>
            <span className="material-symbols-outlined text-[18px]">{saving ? 'progress_activity' : 'save'}</span>
            {saving ? t('settings.models.providers.saving') : t('settings.models.providers.save')}
          </Button>
        </div>
      </div>
    </div>
  )
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="block">
      <span className="block font-label-sm text-on-surface-variant mb-xs">{label}</span>
      {children}
    </label>
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
