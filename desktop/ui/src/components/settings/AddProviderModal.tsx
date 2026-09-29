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

import { useId, useState } from 'react'
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Modal } from '@/components/ui/modal'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import * as api from '@/lib/tauri-api'
import { fetchFailureMessage, testResultMessage } from './models-settings/utils'
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

/// In-modal probe state for the "Test connection" button (review §2-12 /
/// §3-B item 11): spinner while the probe runs, then the categorized
/// verdict + client-side round-trip latency. Purely transient — testing
/// never saves.
type TestState =
  | { status: 'testing' }
  | { status: 'done'; result: api.TestConnectionResult; latencyMs: number }
  | null

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
  // B6-37: which field the error belongs to — drives aria-invalid/aria-describedby.
  const [errorField, setErrorField] = useState<'label' | 'baseUrl' | null>(null)
  // P2: the modal holds unsaved edits (label, key, advanced rows…) — a stray
  // Esc / backdrop click must not silently throw them away once anything is
  // filled in.
  const [confirmDiscard, setConfirmDiscard] = useState(false)
  const dirty =
    label.trim() !== (editing?.display_name ?? '') ||
    kind !== (editing?.kind ?? 'openai-compatible') ||
    baseUrl.trim() !== (editing?.base_url ?? '') ||
    apiKey.trim() !== '' ||
    model.trim() !== ''

  const requestClose = () => {
    if (dirty) {
      setConfirmDiscard(true)
      return
    }
    onClose()
  }

  const info = KIND_INFO[kind] ?? KIND_INFO['openai-compatible']

  // === Fetch model list + in-modal connection test (review §2-9 / §2-12) ===
  const modelListId = useId()
  const [suggestions, setSuggestions] = useState<string[] | null>(null)
  const [fetching, setFetching] = useState(false)
  const [fetchFailure, setFetchFailure] = useState<api.FetchModelsFailure | null>(null)
  const [testState, setTestState] = useState<TestState>(null)

  // A fetched list (and any verdict) is only valid for the kind+base_url+
  // key it was fetched with — any of those changes invalidates it.
  const clearProbeState = () => {
    setSuggestions(null)
    setFetchFailure(null)
    setTestState(null)
  }

  // The stored key counts in edit mode: the backend falls back to the saved
  // credential for this connection id when the input is empty (the modal
  // never re-displays the secret).
  const keyAvailable = apiKey.trim() !== '' || !!editing?.has_api_key
  const canProbe = baseUrl.trim() !== '' && (!info.needsKey || keyAvailable)
  const providerLabel = kindLabel(intl, kind)

  const applyQuickFill = (qf: (typeof QUICK_FILL)[number]) => {
    setKind(qf.kind)
    if (qf.baseUrl) setBaseUrl(qf.baseUrl)
    if (qf.model) setModel(qf.model)
    if (!label) setLabel(qf.id === 'custom' ? '' : qf.label)
    clearProbeState()
  }

  const handleFetchModels = async () => {
    if (!canProbe || fetching) return
    setFetching(true)
    setFetchFailure(null)
    try {
      const models = await api.fetchProviderModels(
        editing?.id ?? null,
        kind,
        baseUrl.trim(),
        apiKey.trim() || null,
      )
      setSuggestions(models)
    } catch (e) {
      setSuggestions(null)
      setFetchFailure(api.parseFetchModelsError(String(e)))
    } finally {
      setFetching(false)
    }
  }

  const handleTestConnection = async () => {
    if (!canProbe || testState?.status === 'testing') return
    setTestState({ status: 'testing' })
    setFetchFailure(null)
    const start = performance.now()
    try {
      const result = await api.testProviderCredentials(
        kind,
        baseUrl.trim() || null,
        apiKey.trim() || null,
        editing?.id ?? null,
      )
      setTestState({ status: 'done', result, latencyMs: Math.max(1, Math.round(performance.now() - start)) })
    } catch (e) {
      setTestState({ status: 'done', result: { kind: 'unknown', message: String(e) }, latencyMs: 0 })
    }
  }

  const submit = async () => {
    const trimmedLabel = label.trim()
    if (!trimmedLabel) {
      setError(t('settings.models.providers.needLabel'))
      setErrorField('label')
      return
    }
    if (info.baseUrlRequired && !baseUrl.trim()) {
      setError(t('settings.models.providers.needBaseUrl'))
      setErrorField('baseUrl')
      return
    }
    setSaving(true)
    setError(null)
    setErrorField(null)
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
      setErrorField(null)
    } finally {
      setSaving(false)
    }
  }

  return (
    <Modal
      open
      onClose={requestClose}
      closeOnEscape={!dirty}
      closeOnBackdrop={!dirty}
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
                className="inline-flex items-center gap-xs px-sm py-xs rounded-lg border border-outline-variant/40 bg-surface-container-low/40 hover:border-primary/40 hover:bg-primary/5 text-on-surface-variant hover:text-primary font-label-sm text-label-sm cursor-pointer"
              >
                <span className="material-symbols-outlined icon-sm">{qf.icon}</span>
                {qf.id === 'custom' ? t(qf.label) : qf.label}
              </Button>
            ))}
          </div>
        </div>

        <div className="space-y-sm">
          <Field label={t('settings.models.providers.labelField')}>
            <Input
              className="w-full px-md py-sm bg-surface text-on-surface border border-outline-variant/50 rounded-lg outline-none focus:ring-2 focus:ring-primary font-body-sm"
              value={label}
              onChange={(e) => { setLabel(e.target.value); setError(null); setErrorField(null) }}
              placeholder={t('settings.models.providers.labelPlaceholder')}
              autoFocus
              aria-invalid={errorField === 'label' || undefined}
              aria-describedby={error ? 'add-provider-error' : undefined}
            />
          </Field>

          <Field label={t('settings.models.providers.kindField')}>
            <select
              className="w-full px-md py-sm bg-surface text-on-surface border border-outline-variant/50 rounded-lg outline-none focus:ring-2 focus:ring-primary font-body-sm cursor-pointer"
              value={kind}
              onChange={(e) => { setKind(e.target.value); clearProbeState() }}
            >
              {Object.keys(KIND_INFO).map(k => (
                <option key={k} value={k}>{kindLabel(intl, k)}</option>
              ))}
            </select>
          </Field>

          <Field label={t(info.baseUrlRequired ? 'settings.models.providers.baseUrlRequired' : 'settings.models.providers.baseUrlOptional')}>
            <Input
              className="w-full px-md py-sm bg-surface text-on-surface border border-outline-variant/50 rounded-lg outline-none focus:ring-2 focus:ring-primary font-body-sm font-mono"
              value={baseUrl}
              onChange={(e) => { setBaseUrl(e.target.value); setError(null); setErrorField(null); clearProbeState() }}
              placeholder="https://api.example.com/v1"
              aria-invalid={errorField === 'baseUrl' || undefined}
              aria-describedby={error ? 'add-provider-error' : undefined}
            />
          </Field>

          <Field label={t('settings.models.providers.apiKeyField')}>
            <Input
              className="w-full px-md py-sm bg-surface text-on-surface border border-outline-variant/50 rounded-lg outline-none focus:ring-2 focus:ring-primary font-body-sm font-mono"
              type="password"
              value={apiKey}
              onChange={(e) => { setApiKey(e.target.value); setError(null); clearProbeState() }}
              placeholder={editing ? t('settings.models.providers.apiKeyKeep') : t('settings.models.providers.apiKeyPlaceholder')}
              disabled={!info.needsKey}
            />
          </Field>

          {/* Review §2-12: verify the form values BEFORE saving — save ≠
              test. Edit mode with an untouched key tests the stored
              credential (api_key null + provider id → backend reads the
              credential store). */}
          <div className="flex items-center gap-md flex-wrap">
            <Button
              type="button"
              variant="outline"
              data-testid="test-provider-connection"
              disabled={!canProbe || testState?.status === 'testing'}
              onClick={handleTestConnection}
              className="inline-flex items-center gap-xs px-md py-xs rounded-lg border border-outline-variant/50 bg-surface-container-low hover:border-primary/40 hover:bg-primary/5 text-on-surface font-label-sm cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <span className={`material-symbols-outlined icon-md${testState?.status === 'testing' ? ' animate-spin' : ''}`}>
                {testState?.status === 'testing' ? 'progress_activity' : 'network_check'}
              </span>
              {testState?.status === 'testing'
                ? t('settings.models.providers.testingConnection')
                : t('settings.models.providers.testConnection')}
            </Button>
            {testState?.status === 'done' && (
              <p
                role="status"
                data-testid="provider-test-status"
                className={`font-label-sm ${testState.result.kind === 'success' ? 'text-primary' : 'text-error'}`}
              >
                {testState.result.kind === 'success'
                  ? intl.formatMessage({ id: 'settings.models.testResult.successLatency' }, { ms: testState.latencyMs })
                  : testResultMessage(intl, testState.result, providerLabel)}
              </p>
            )}
          </div>

          <Field label={t('settings.models.providers.modelField')}>
            <div className="flex gap-sm items-center">
              <Input
                className="w-full px-md py-sm bg-surface text-on-surface border border-outline-variant/50 rounded-lg outline-none focus:ring-2 focus:ring-primary font-body-sm font-mono"
                value={model}
                onChange={(e) => setModel(e.target.value)}
                placeholder="claude-sonnet-4-6"
                // Conditional: an input with a `list` attribute is exposed
                // to assistive tech as a combobox, which would mislabel the
                // free-text field (and collide with the kind select's role
                // in tests) when no suggestions exist.
                list={suggestions != null && suggestions.length > 0 ? modelListId : undefined}
                data-testid="provider-model-input"
              />
              {/* Review §2-9: pull the live /models catalog so users pick
                  real ids instead of typing free text and discovering typos
                  as provider 404s. Free text stays valid — the datalist only
                  suggests. Works identically in add and edit mode (edit
                  falls back to the stored key). */}
              <Button
                type="button"
                variant="outline"
                data-testid="fetch-models"
                disabled={!canProbe || fetching}
                onClick={handleFetchModels}
                className="shrink-0 inline-flex items-center gap-xs px-md py-sm rounded-lg border border-outline-variant/50 bg-surface-container-low hover:border-primary/40 hover:bg-primary/5 text-on-surface font-label-sm cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed"
              >
                <span className={`material-symbols-outlined icon-md${fetching ? ' animate-spin' : ''}`}>
                  {fetching ? 'progress_activity' : 'cloud_download'}
                </span>
                {fetching
                  ? t('settings.models.providers.fetchingModels')
                  : t('settings.models.providers.fetchModels')}
              </Button>
            </div>
            {suggestions != null && suggestions.length > 0 && (
              <>
                <datalist id={modelListId}>
                  {suggestions.map((m) => <option key={m} value={m} />)}
                </datalist>
                <p data-testid="models-found" className="mt-xs font-label-sm text-on-surface-variant">
                  {intl.formatMessage({ id: 'settings.models.providers.modelsFound' }, { count: suggestions.length })}
                </p>
              </>
            )}
            {suggestions != null && suggestions.length === 0 && (
              <p data-testid="models-empty" className="mt-xs font-label-sm text-on-surface-variant">
                {t('settings.models.providers.modelsEmpty')}
              </p>
            )}
            {fetchFailure && (
              <p role="alert" data-testid="fetch-models-error" className="mt-xs font-label-sm text-error">
                {fetchFailureMessage(intl, fetchFailure, providerLabel)}
              </p>
            )}
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
              <span className="material-symbols-outlined icon-md">{advancedOpen ? 'expand_less' : 'expand_more'}</span>
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
          <div id="add-provider-error" role="alert" className="font-label-sm text-label-sm text-error">{error}</div>
        ) : null}

        <div className="flex justify-end gap-sm pt-xs">
          <Button className="px-md py-sm border border-outline-variant bg-surface-container-lowest text-on-surface font-label-md rounded-lg hover:bg-surface-container cursor-pointer" onClick={requestClose}>
            {t('settings.models.providers.cancel')}
          </Button>
          <Button className="px-lg py-sm bg-primary text-on-primary font-label-md rounded-lg hover:bg-primary/90 transition-colors flex items-center gap-sm cursor-pointer disabled:opacity-50" onClick={submit} disabled={saving}>
            <span className="material-symbols-outlined icon-md">{saving ? 'progress_activity' : 'save'}</span>
            {saving ? t('settings.models.providers.saving') : t('settings.models.providers.save')}
          </Button>
        </div>
      </div>

      <ConfirmDialog
        open={confirmDiscard}
        title={t('ui.modal.discard.title')}
        message={t('ui.modal.discard.message')}
        confirmLabel={t('ui.modal.discard.confirm')}
        cancelLabel={t('ui.modal.discard.cancel')}
        destructive
        onConfirm={onClose}
        onCancel={() => setConfirmDiscard(false)}
      />
    </Modal>
  )
}

// Re-export the union type for callers that import from this module.
export type { ProviderKind } from '@/types'
