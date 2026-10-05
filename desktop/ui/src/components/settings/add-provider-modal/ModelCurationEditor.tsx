// S2-1 (模型仓固化) — multi-select editor for the fetched model list.
//
// The 90% curation flow: Fetch models pulls the endpoint's real catalog,
// the user ticks the models they actually use, and Save固ies the selection
// into the provider slot's `models: Vec<ModelSpec>` (providers.toml v2).
//
// Guardrails (裁定⑥, pinned in `modelCuration.ts` + its vitest):
//   - default zero selected (initial selection comes from the parent — an
//     edit starts from the provider's existing declarations),
//   - soft cap 50: a longer list renders the over-cap warning and the
//     "select all" button refuses,
//   - within the cap, "select all" opens a confirm dialog first.

import { useState } from 'react'
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import {
  MODEL_VAULT_SOFT_CAP,
  isOverCap,
  planSelectAll,
  toggled,
} from './modelCuration'

export interface ModelCurationEditorProps {
  /** The fetched endpoint catalog (`Fetch models` result). */
  suggestions: string[]
  /** The current selection (curated ids — may include ids not in the
   *  current fetch, e.g. an existing declaration the refetch dropped). */
  selected: Set<string>
  onChange: (next: Set<string>) => void
}

export default function ModelCurationEditor({
  suggestions,
  selected,
  onChange,
}: ModelCurationEditorProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const [confirmSelectAll, setConfirmSelectAll] = useState(false)

  const overCap = isOverCap(suggestions.length)
  const decision = planSelectAll(suggestions)

  return (
    <div
      data-testid="model-curation"
      className="mt-sm p-md rounded-lg border border-outline-variant/30 bg-surface-container-low/40"
    >
      <div className="flex items-center justify-between gap-sm flex-wrap mb-sm">
        <p className="font-label-sm text-on-surface-variant">
          {intl.formatMessage(
            { id: 'settings.models.modelCuration.selectedCount' },
            { selected: selected.size, total: suggestions.length },
          )}
        </p>
        <div className="flex items-center gap-xs">
          <Button
            type="button"
            variant="ghost"
            data-testid="curation-select-all"
            disabled={overCap || selected.size === suggestions.length}
            onClick={() => {
              // 裁定⑥: select-all always confirms — and beyond the soft cap
              // it is refused outright (planSelectAll is the pinned policy).
              if (planSelectAll(suggestions).ok) setConfirmSelectAll(true)
            }}
            className="inline-flex items-center gap-xs px-sm py-xs rounded-lg font-label-sm text-on-surface-variant hover:text-primary cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed"
          >
            <span className="material-symbols-outlined icon-sm" aria-hidden="true">
              select_all
            </span>
            {t('settings.models.modelCuration.selectAll')}
          </Button>
          <Button
            type="button"
            variant="ghost"
            data-testid="curation-clear"
            disabled={selected.size === 0}
            onClick={() => onChange(new Set())}
            className="inline-flex items-center gap-xs px-sm py-xs rounded-lg font-label-sm text-on-surface-variant hover:text-primary cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed"
          >
            <span className="material-symbols-outlined icon-sm" aria-hidden="true">
              clear_all
            </span>
            {t('settings.models.modelCuration.clear')}
          </Button>
        </div>
      </div>

      <p className="font-label-sm text-on-surface-variant mb-sm">
        {t('settings.models.modelCuration.hint')}
      </p>

      {overCap && (
        <p
          role="alert"
          data-testid="curation-over-cap"
          className="mb-sm font-label-sm text-on-surface-variant flex items-center gap-xs"
        >
          <span className="material-symbols-outlined icon-sm text-warning" aria-hidden="true">
            warning
          </span>
          {intl.formatMessage(
            { id: 'settings.models.modelCuration.overCap' },
            { cap: MODEL_VAULT_SOFT_CAP, total: suggestions.length },
          )}
        </p>
      )}

      <ul
        data-testid="curation-list"
        className="max-h-64 overflow-y-auto rounded-md border border-outline-variant/20 divide-y divide-outline-variant/20 bg-surface"
      >
        {suggestions.map((id) => (
          <li key={id}>
            <label className="flex items-center gap-sm px-md py-xs cursor-pointer hover:bg-surface-container-high/50">
              <input
                type="checkbox"
                data-testid="curation-item"
                checked={selected.has(id)}
                onChange={(e) => onChange(toggled(selected, id, e.target.checked))}
                className="accent-primary cursor-pointer"
              />
              <span className="font-mono text-label-sm text-on-surface truncate">{id}</span>
            </label>
          </li>
        ))}
      </ul>

      <ConfirmDialog
        open={confirmSelectAll}
        title={t('settings.models.modelCuration.confirmAllTitle')}
        message={intl.formatMessage(
          { id: 'settings.models.modelCuration.confirmAllMessage' },
          { count: decision.ok ? decision.ids.length : 0 },
        )}
        confirmLabel={t('settings.models.modelCuration.confirmAllConfirm')}
        cancelLabel={t('settings.models.modelCuration.confirmAllCancel')}
        onConfirm={() => {
          if (decision.ok) onChange(new Set(decision.ids))
          setConfirmSelectAll(false)
        }}
        onCancel={() => setConfirmSelectAll(false)}
      />
    </div>
  )
}
