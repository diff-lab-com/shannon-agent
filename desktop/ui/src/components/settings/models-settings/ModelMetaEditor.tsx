// S2-2 (per-model metadata editor) — the inline declaration form rendered
// under an expanded catalog row in Settings → Models.
//
// Scope: the four metadata numbers the engine consumes today — context
// window (compaction budget), max output (S2-3 request clamp), the two
// per-million prices (billing, both halves required) — plus the
// vision / tool-calling capability bits (tier badge + vision gate). Blank
// numeric input clears the declaration field (honest fallback to the
// catalog), noted inline.
//
// Validation mirrors the engine (`ModelSpec::validate`): invalid input keeps
// the command from ever firing — errors render inline and the Save button
// submits nothing. The parent owns the API call; `onSave` receives a
// validated, complete `DeclaredModelInput`.
//
// The "declared pricing wins" note is deliberate: providers.toml v2
// declarations are authoritative over the built-in catalog once BOTH prices
// are set (the engine's billing pin) — users correcting a wrong catalog
// price should know their number takes precedence.

import { useState } from 'react'
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import type { DeclaredModelInput, DeclaredModelSpec } from '@/types'
import {
  type ModelMetaDraft,
  type ModelMetaErrors,
  draftFromSpec,
  draftToInput,
  hasErrors,
  validateDraft,
} from './modelMeta'

export interface ModelMetaEditorProps {
  /** The model id exactly as sent to the API (the declaration key). */
  modelId: string
  /** The current declaration, when the row already sits in the vault. */
  spec?: DeclaredModelSpec | null
  /** True while the parent's `set_provider_models` call is in flight. */
  saving?: boolean
  /** Called only with a validated input — invalid drafts never leave. */
  onSave: (input: DeclaredModelInput) => void
  onCancel: () => void
}

export default function ModelMetaEditor({
  modelId,
  spec,
  saving = false,
  onSave,
  onCancel,
}: ModelMetaEditorProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const [draft, setDraft] = useState<ModelMetaDraft>(() => draftFromSpec(spec))
  const [errors, setErrors] = useState<ModelMetaErrors>({})

  const set = (patch: Partial<ModelMetaDraft>) => setDraft((d) => ({ ...d, ...patch }))

  const tokenField = (
    field: 'contextWindow' | 'maxOutput',
    labelId: string,
    testId: string,
  ) => {
    const error = errors[field]
    return (
      <div>
        <label htmlFor={`${testId}-input`} className="font-label-sm text-on-surface-variant mb-xs block">
          {t(labelId)}
        </label>
        <Input
          id={`${testId}-input`}
          data-testid={testId}
          type="number"
          min={1}
          step={1}
          value={draft[field]}
          onChange={(e) => set({ [field]: e.target.value })}
          aria-invalid={!!error}
          aria-describedby={error ? `${testId}-error` : undefined}
          className="w-44 px-sm py-xs bg-surface text-on-surface border border-outline-variant/50 rounded-sm font-body-sm font-mono"
        />
        {error && (
          <p id={`${testId}-error`} role="alert" data-testid={`${testId}-error`} className="mt-xs text-label-sm text-error">
            {t(error)}
          </p>
        )}
      </div>
    )
  }

  const priceField = (field: 'priceIn' | 'priceOut', labelId: string, testId: string) => {
    const error = errors[field]
    return (
      <div>
        <label htmlFor={`${testId}-input`} className="font-label-sm text-on-surface-variant mb-xs block">
          {t(labelId)}
        </label>
        <Input
          id={`${testId}-input`}
          data-testid={testId}
          type="number"
          min={0}
          step="any"
          value={draft[field]}
          onChange={(e) => set({ [field]: e.target.value })}
          aria-invalid={!!error}
          aria-describedby={error ? `${testId}-error` : undefined}
          className="w-44 px-sm py-xs bg-surface text-on-surface border border-outline-variant/50 rounded-sm font-body-sm font-mono"
        />
        {error && (
          <p id={`${testId}-error`} role="alert" data-testid={`${testId}-error`} className="mt-xs text-label-sm text-error">
            {t(error)}
          </p>
        )}
      </div>
    )
  }

  const capBox = (field: 'vision' | 'tools', labelId: string, testId: string) => (
    <label className="flex items-center gap-sm cursor-pointer">
      <input
        type="checkbox"
        data-testid={testId}
        checked={draft[field]}
        onChange={(e) => set({ [field]: e.target.checked })}
        className="accent-primary cursor-pointer"
      />
      <span className="font-label-sm text-on-surface">{t(labelId)}</span>
    </label>
  )

  const submit = () => {
    const found = validateDraft(draft)
    if (hasErrors(found)) {
      setErrors(found)
      return
    }
    setErrors({})
    onSave(draftToInput(modelId, draft, spec))
  }

  return (
    <div
      data-testid="model-meta-editor"
      className="mx-md mb-md p-md rounded-lg border border-outline-variant/30 bg-surface-container-low/40"
    >
      <div className="flex items-center justify-between gap-sm flex-wrap mb-sm">
        <div>
          <p className="font-label-md text-on-surface font-bold">{t('settings.models.vault.editorTitle')}</p>
          <p className="font-label-sm text-on-surface-variant">{t('settings.models.vault.editorSubtitle')}</p>
        </div>
        <span className="font-mono text-label-sm text-on-surface-variant px-sm py-xs rounded-md bg-surface-container-high/60 truncate max-w-full">
          {modelId}
        </span>
      </div>

      <div className="grid grid-cols-1 sm:grid-cols-2 gap-md max-w-xl">
        {tokenField('contextWindow', 'settings.models.vault.contextWindow', 'meta-editor-context')}
        {tokenField('maxOutput', 'settings.models.vault.maxOutput', 'meta-editor-max-output')}
        {priceField('priceIn', 'settings.models.vault.priceIn', 'meta-editor-price-in')}
        {priceField('priceOut', 'settings.models.vault.priceOut', 'meta-editor-price-out')}
      </div>

      <div className="flex items-center gap-lg flex-wrap mt-md">
        {capBox('vision', 'settings.models.vault.vision', 'meta-editor-vision')}
        {capBox('tools', 'settings.models.vault.tools', 'meta-editor-tools')}
      </div>

      <div className="mt-md space-y-xs">
        <p className="font-label-sm text-on-surface-variant flex items-start gap-xs">
          <span className="material-symbols-outlined icon-sm shrink-0" aria-hidden="true">info</span>
          {t('settings.models.vault.blankHint')}
        </p>
        <p className="font-label-sm text-on-surface-variant flex items-start gap-xs">
          <span className="material-symbols-outlined icon-sm shrink-0" aria-hidden="true">paid</span>
          {t('settings.models.vault.pricingNote')}
        </p>
        <p className="font-label-sm text-on-surface-variant flex items-start gap-xs">
          <span className="material-symbols-outlined icon-sm shrink-0" aria-hidden="true">fact_check</span>
          {t('settings.models.vault.capabilityNote')}
        </p>
      </div>

      <div className="flex items-center gap-sm mt-md">
        <Button
          type="button"
          data-testid="meta-editor-save"
          disabled={saving}
          onClick={submit}
          className="inline-flex items-center gap-xs px-md py-xs rounded-lg bg-primary text-on-primary font-label-md cursor-pointer disabled:opacity-60"
        >
          {saving ? t('settings.models.vault.saving') : t('settings.models.vault.save')}
        </Button>
        <Button
          type="button"
          variant="ghost"
          data-testid="meta-editor-cancel"
          disabled={saving}
          onClick={onCancel}
          className="px-md py-xs rounded-lg font-label-md text-on-surface-variant hover:bg-surface-container-high cursor-pointer"
        >
          {t('settings.models.vault.cancel')}
        </Button>
      </div>
    </div>
  )
}
