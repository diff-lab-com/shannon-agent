// Add/Edit Provider modal — orchestrator only (T3.1).
//
// SCOPE: Modal for creating a new LLM provider connection or editing an
// existing one. The modal is opened from `ModelsSettings → Providers` and
// from the first-run Welcome flow. All mutation goes through
// `api.saveProvider(input)` and the parent refreshes providers from the
// returned `ProvidersFile` snapshot.
//
// T3.1 split:
//   - `add-provider-modal/types.ts` — `KindInfo`, `KIND_INFO`, `QuickFill`,
//     `QUICK_FILL`, `kindLabel`, `HeaderRow`, `AdvancedState`,
//     `advancedFromEditing`, `headersToRecord`, `parseDefaultMaxTokens`.
//   - `add-provider-modal/Field.tsx` — labeled form field helper.
//   - `add-provider-modal/HeaderRowsEditor.tsx` — extra-headers rows.
//   - `add-provider-modal/DefaultMaxTokensField.tsx` — numeric override.
//   - `add-provider-modal/TiersEditor.tsx` — per-tier model overrides.
//   - `add-provider-modal/FallbackModelsEditor.tsx` — fallback list.

import { useState } from 'react'
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Modal } from '@/components/ui/modal'
import * as api from '@/lib/tauri-api'
import type {
  ProviderConnection,
  ProviderInput,
  ProvidersFile,
} from '@/types'
import { DefaultMaxTokensField } from './add-provider-modal/DefaultMaxTokensField'
import { FallbackModelsEditor } from './add-provider-modal/FallbackModelsEditor'
import { Field } from './add-provider-modal/Field'
import { HeaderRowsEditor } from './add-provider-modal/HeaderRowsEditor'
import { TiersEditor } from './add-provider-modal/TiersEditor'
import {
  KIND_INFO,
  QUICK_FILL,
  advancedFromEditing,
  headersToRecord,
  kindLabel,
  parseDefaultMaxTokens,
  type AdvancedState,
} from './add-provider-modal/types'

export interface AddProviderModalProps {
  editing: ProviderConnection | null
  onClose: () => void
  onSaved: (f: ProvidersFile) => void
}

export default function AddProviderModal({ editing, onClose, onSaved }: AddProviderModalProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const [label, setLabel] = useState(editing?.display_name ?? '')
  const [kind, setKind] = useState<string>(editing?.kind ?? 'openai-compatible')
  const [baseUrl, setBaseUrl] = useState(editing?.base_url ?? '')
  const [apiKey, setApiKey] = useState('')
  const [model, setModel] = useState('')
  const [advanced, setAdvanced] = useState<AdvancedState>(() => advancedFromEditing(editing))
  const [advancedOpen, setAdvancedOpen] = useState(false)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const info = KIND_INFO[kind] ?? KIND_INFO['openai-compatible']

  const applyQuickFill = (qf: (typeof QUICK_FILL)[number]) => {
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
      display_name: trimmedLabel,
      kind: kind,
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
      fallback_models: advanced.fallbackModels.map((m) => m.trim()).filter(Boolean),
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
    <Modal
      open
      onClose={onClose}
      size="2xl"
      title={editing ? t('settings.models.providers.editTitle') : t('settings.models.providers.addTitle')}
      className="max-h-[90vh] overflow-y-auto p-lg space-y-md"
    >
      <div data-testid="add-provider-modal" className="space-y-md">


        {/* Quick fill */}
        <div>
          <p className="font-label-sm text-on-surface-variant mb-xs">{t('settings.models.providers.quickFill')}</p>
          <div className="flex flex-wrap gap-xs">
            {QUICK_FILL.map(qf => (
              <Button
                key={qf.id}
                type="button"
                variant="outline"
                onClick={() => applyQuickFill(qf)}
                className="inline-flex items-center gap-xs px-sm py-xs rounded-lg border border-outline-variant/40 bg-surface-container-low/40 hover:border-primary/40 hover:bg-primary/5 text-on-surface-variant hover:text-primary font-label-sm text-[12px] cursor-pointer"
              >
                <span className="material-symbols-outlined text-[14px]">{qf.icon}</span>
                {qf.id === 'custom' ? t(qf.label) : qf.label}
              </Button>
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
            <Button
              type="button"
              variant="ghost"
              onClick={() => setAdvancedOpen((v) => !v)}
              className="inline-flex items-center gap-xs font-label-sm text-on-surface-variant hover:text-primary cursor-pointer"
              aria-expanded={advancedOpen}
              data-testid="add-provider-advanced-toggle"
            >
              <span className="material-symbols-outlined text-[18px]">{advancedOpen ? 'expand_less' : 'expand_more'}</span>
              {t('settings.models.providers.advanced')}
            </Button>
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
                  activeModelId={model.trim()}
                  onChange={(tiers) => setAdvanced((s) => ({ ...s, tiers }))}
                />
                <FallbackModelsEditor
                  models={advanced.fallbackModels}
                  onChange={(fallbackModels) => setAdvanced((s) => ({ ...s, fallbackModels }))}
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
    </Modal>
  )
}

// Re-export the union type for callers that import from this module.
export type { ProviderKind } from '@/types'
