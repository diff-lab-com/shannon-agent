import { useState } from 'react'
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import * as api from '@/lib/tauri-api'
import type {
  ProviderConnection,
  ProviderInput,
  ProviderKind,
  ProvidersFile,
} from '@/types'

export interface AddProviderModalProps {
  editing: ProviderConnection | null
  onClose: () => void
  onSaved: (f: ProvidersFile) => void
}

interface KindInfo {
  labelKey: string
  icon: string
  baseUrlRequired: boolean
  needsKey: boolean
}

export const KIND_INFO: Record<string, KindInfo> = {
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
export const QUICK_FILL: QuickFill[] = [
  { id: 'anthropic', label: 'Anthropic', icon: 'auto_awesome', kind: 'anthropic', model: 'claude-sonnet-4-6' },
  { id: 'openai', label: 'OpenAI', icon: 'bolt', kind: 'openai', model: 'gpt-4.1-mini' },
  { id: 'deepseek', label: 'DeepSeek', icon: 'psychology', kind: 'deepseek', model: 'deepseek-chat' },
  { id: 'glm', label: 'GLM (Zhipu)', icon: 'auto_awesome', kind: 'openai-compatible', baseUrl: 'https://open.bigmodel.cn/api/paas/v4', model: 'glm-4-plus' },
  { id: 'kimi', label: 'Kimi (Moonshot)', icon: 'dark_mode', kind: 'openai-compatible', baseUrl: 'https://api.moonshot.cn/v1', model: 'moonshot-v1-8k' },
  { id: 'minimax', label: 'MiniMax', icon: 'group', kind: 'openai-compatible', baseUrl: 'https://api.minimax.chat/v1', model: 'abab6.5s-chat' },
  { id: 'ollama', label: 'Ollama (local)', icon: 'dns', kind: 'ollama', baseUrl: 'http://localhost:11434', model: 'llama3.2' },
  { id: 'custom', label: 'settings.models.providers.customOpenAI', icon: 'hub', kind: 'openai-compatible' },
]

export function kindLabel(intl: ReturnType<typeof useIntl>, kind: string): string {
  return intl.formatMessage({ id: KIND_INFO[kind]?.labelKey ?? 'settings.models.providers.kinds.openaiCompatible' })
}

/// Key/value pair for the `extra_headers` advanced row. An empty `key` row
/// is silently dropped at submit time so the engine never sees `""`.
interface HeaderRow {
  key: string
  value: string
}

/// Internal state for the advanced disclosure. Kept as plain state — the
/// payload only needs the final `Record<string, string>`, `number | null`,
/// and `Tiers` shape; this view-model exists to keep the JSX simple.
interface AdvancedState {
  headers: HeaderRow[]
  defaultMaxTokensInput: string
  tiers: { fast: string; standard: string; pro: string }
}

function advancedFromEditing(editing: ProviderConnection | null): AdvancedState {
  if (!editing) {
    return {
      headers: [],
      defaultMaxTokensInput: '',
      tiers: { fast: '', standard: '', pro: '' },
    }
  }
  return {
    headers: Object.entries(editing.extra_headers ?? {}).map(([key, value]) => ({ key, value })),
    defaultMaxTokensInput: editing.default_max_tokens == null ? '' : String(editing.default_max_tokens),
    tiers: {
      fast: editing.tiers?.fast ?? '',
      standard: editing.tiers?.standard ?? '',
      pro: editing.tiers?.pro ?? '',
    },
  }
}

function headersToRecord(rows: HeaderRow[]): Record<string, string> {
  const out: Record<string, string> = {}
  for (const r of rows) {
    const k = r.key.trim()
    if (!k) continue
    out[k] = r.value
  }
  return out
}

function parseDefaultMaxTokens(s: string): number | null {
  const trimmed = s.trim()
  if (!trimmed) return null
  const n = Number(trimmed)
  if (!Number.isFinite(n) || n <= 0) return null
  return Math.floor(n)
}

export default function AddProviderModal({ editing, onClose, onSaved }: AddProviderModalProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const [label, setLabel] = useState(editing?.label ?? '')
  const [kind, setKind] = useState<string>(editing?.provider_kind ?? 'openai-compatible')
  const [baseUrl, setBaseUrl] = useState(editing?.base_url ?? '')
  const [apiKey, setApiKey] = useState('')
  const [model, setModel] = useState(editing?.model ?? '')
  const [advanced, setAdvanced] = useState<AdvancedState>(() => advancedFromEditing(editing))
  const [advancedOpen, setAdvancedOpen] = useState(false)
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
      // Phase 2 task 3: surface the v2 ProviderProfile fields. Empty rows /
      // empty inputs collapse to `null` or omitted so the engine applies
      // its own defaults (A1 — never send empty strings as overrides).
      extra_headers: headersToRecord(advanced.headers),
      default_max_tokens: parseDefaultMaxTokens(advanced.defaultMaxTokensInput),
      tiers: {
        fast: advanced.tiers.fast.trim() || null,
        standard: advanced.tiers.standard.trim() || null,
        pro: advanced.tiers.pro.trim() || null,
      },
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
        className="bg-surface-container-lowest border border-outline-variant/40 rounded-2xl shadow-xl w-full max-w-lg p-lg space-y-md max-h-[90vh] overflow-y-auto"
        onClick={(e) => e.stopPropagation()}
        data-testid="add-provider-modal"
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

          {/* Advanced disclosure — surfaces v2 ProviderProfile fields. The
              bare fields above are the 90% path; advanced is for users who
              need to tweak per-provider behavior. */}
          <div className="pt-xs">
            <button
              type="button"
              onClick={() => setAdvancedOpen((v) => !v)}
              className="inline-flex items-center gap-xs font-label-sm text-on-surface-variant hover:text-primary cursor-pointer"
              aria-expanded={advancedOpen}
              data-testid="add-provider-advanced-toggle"
            >
              <span className="material-symbols-outlined text-[18px]">{advancedOpen ? 'expand_less' : 'expand_more'}</span>
              {t('settings.models.providers.advanced')}
            </button>
            {advancedOpen ? (
              <div className="mt-sm space-y-md p-md rounded-lg border border-outline-variant/30 bg-surface-container-low/40">
                <HeaderRowsEditor
                  rows={advanced.headers}
                  onChange={(rows) => setAdvanced((s) => ({ ...s, headers: rows }))}
                />
                <DefaultMaxTokensField
                  value={advanced.defaultMaxTokensInput}
                  onChange={(v) => setAdvanced((s) => ({ ...s, defaultMaxTokensInput: v }))}
                />
                <TiersEditor
                  tiers={advanced.tiers}
                  onChange={(tiers) => setAdvanced((s) => ({ ...s, tiers }))}
                />
              </div>
            ) : null}
          </div>
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

function HeaderRowsEditor({
  rows,
  onChange,
}: {
  rows: HeaderRow[]
  onChange: (rows: HeaderRow[]) => void
}) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  return (
    <div>
      <p className="font-label-sm text-on-surface-variant mb-xs">{t('settings.models.providers.extraHeaders')}</p>
      {rows.length === 0 ? (
        <p className="font-label-xs text-on-surface-variant opacity-60 mb-xs" data-testid="extra-headers-empty">
          {t('settings.models.providers.extraHeadersEmpty')}
        </p>
      ) : null}
      <div className="space-y-xs">
        {rows.map((row, i) => (
          <div key={i} className="flex items-center gap-xs" data-testid="extra-headers-row">
            <Input
              className="flex-1 px-sm py-xs bg-surface text-on-surface border border-outline-variant/50 rounded font-body-xs font-mono"
              value={row.key}
              placeholder={t('settings.models.providers.extraHeadersKey')}
              onChange={(e) => {
                const next = rows.slice()
                next[i] = { ...row, key: e.target.value }
                onChange(next)
              }}
            />
            <Input
              className="flex-1 px-sm py-xs bg-surface text-on-surface border border-outline-variant/50 rounded font-body-xs font-mono"
              value={row.value}
              placeholder={t('settings.models.providers.extraHeadersValue')}
              onChange={(e) => {
                const next = rows.slice()
                next[i] = { ...row, value: e.target.value }
                onChange(next)
              }}
            />
            <Button
              variant="ghost"
              type="button"
              className="px-sm py-xs text-on-surface-variant hover:text-error cursor-pointer"
              onClick={() => onChange(rows.filter((_, j) => j !== i))}
              aria-label={t('settings.models.providers.extraHeadersRemove')}
            >
              <span className="material-symbols-outlined text-[16px]">close</span>
            </Button>
          </div>
        ))}
      </div>
      <Button
        variant="ghost"
        type="button"
        className="mt-xs px-sm py-xs font-label-sm text-primary hover:bg-primary/10 cursor-pointer"
        onClick={() => onChange([...rows, { key: '', value: '' }])}
        data-testid="extra-headers-add"
      >
        <span className="material-symbols-outlined text-[16px] mr-xs">add</span>
        {t('settings.models.providers.extraHeadersAdd')}
      </Button>
    </div>
  )
}

function DefaultMaxTokensField({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  return (
    <div>
      <p className="font-label-sm text-on-surface-variant mb-xs">{t('settings.models.providers.defaultMaxTokens')}</p>
      <Input
        className="w-40 px-sm py-xs bg-surface text-on-surface border border-outline-variant/50 rounded font-body-sm font-mono"
        type="number"
        min={1}
        max={200000}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        data-testid="default-max-tokens"
      />
    </div>
  )
}

function TiersEditor({
  tiers,
  onChange,
}: {
  tiers: { fast: string; standard: string; pro: string }
  onChange: (t: { fast: string; standard: string; pro: string }) => void
}) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const rows: Array<{ key: 'fast' | 'standard' | 'pro'; labelKey: string }> = [
    { key: 'fast', labelKey: 'settings.models.addProvider.tierFast' },
    { key: 'standard', labelKey: 'settings.models.addProvider.tierStandard' },
    { key: 'pro', labelKey: 'settings.models.addProvider.tierPro' },
  ]
  return (
    <div>
      <p className="font-label-sm text-on-surface-variant mb-xs">{t('settings.models.providers.tiers')}</p>
      <div className="space-y-xs">
        {rows.map(({ key, labelKey }) => (
          <label key={key} className="flex items-center gap-sm" data-testid={`tier-${key}-row`}>
            <span className="font-label-sm text-on-surface-variant w-20">{t(labelKey)}</span>
            <Input
              className="flex-1 px-sm py-xs bg-surface text-on-surface border border-outline-variant/50 rounded font-body-xs font-mono"
              value={tiers[key]}
              placeholder="model-id"
              onChange={(e) => onChange({ ...tiers, [key]: e.target.value })}
              data-testid={`tier-${key}-input`}
            />
          </label>
        ))}
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

// Re-export the union type for callers that import from this module.
export type { ProviderKind }