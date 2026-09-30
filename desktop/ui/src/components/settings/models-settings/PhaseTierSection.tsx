import { useState } from 'react'
import { useIntl, type PrimitiveType } from 'react-intl'
import { useCatalog } from '@/context/CatalogContext'
import * as api from '@/lib/tauri-api'
import { toastError } from '@/lib/errorToast'
import {
  normalizePhaseTierPref,
  PHASE_TIER_PREFS,
  phaseTierConfigValue,
  phaseTierLabelKey,
  resolveTierModel,
  type PhaseTierPref,
} from '@/lib/phaseTier'

/**
 * R3-3 — Settings → Models "Plan / Act model tiers": the two global
 * dropdowns backing the same `plan_tier` / `act_tier` desktop-config keys
 * the chat header's compact pair writes. Each phase inherits the global
 * default until pinned; the session override (R2-1) beats both at query
 * time. Under each dropdown a display line shows which catalog model the
 * tier resolves to (the same picker list + tier labels, cheapest-wins
 * tie-break — see `lib/phaseTier.ts`).
 *
 * Selection state follows the persisted config (a failed write snaps back
 * via the config refresh — same contract as the strategy pills). Native
 * `<select>` — the settings modals' pattern (AdvancedSettings session-GC
 * retention), jsdom-stable for tests.
 */

function PhaseSelect({
  phase,
  label,
  value,
  models,
  disabled,
  onPick,
  t,
}: {
  phase: 'plan' | 'act'
  label: string
  value: PhaseTierPref
  models: Parameters<typeof resolveTierModel>[1]
  disabled: boolean
  onPick: (phase: 'plan' | 'act', pref: PhaseTierPref) => void
  t: (id: string, values?: Record<string, PrimitiveType>) => string
}) {
  const resolved = value === 'inherit' ? null : resolveTierModel(value, models)
  const selectId = `phase-tier-select-${phase}`
  return (
    <div className="flex-1 min-w-[220px]">
      <label htmlFor={selectId} className="font-label-md text-on-surface font-bold block mb-xs">
        {label}
      </label>
      <select
        id={selectId}
        data-testid={`phase-tier-select-${phase}`}
        disabled={disabled}
        className="w-full px-md py-sm bg-surface text-on-surface border border-outline-variant/50 rounded-lg outline-none focus:ring-2 focus:ring-primary font-body-sm cursor-pointer disabled:opacity-60"
        value={value}
        onChange={(e) => onPick(phase, e.target.value as PhaseTierPref)}
      >
        {PHASE_TIER_PREFS.map((pref) => (
          <option key={pref} value={pref}>
            {t(phaseTierLabelKey(pref))}
          </option>
        ))}
      </select>
      <p className="mt-xs text-label-sm text-on-surface-variant" data-testid={`phase-tier-resolved-${phase}`}>
        {value === 'inherit'
          ? t('settings.models.phaseTier.inheritHint')
          : resolved
            ? t('settings.models.phaseTier.resolved', { model: resolved })
            : t('settings.models.phaseTier.unresolved')}
      </p>
    </div>
  )
}

export function PhaseTierSection() {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, PrimitiveType>) => intl.formatMessage({ id }, values)
  const { config, models, refreshConfig } = useCatalog()
  const [busy, setBusy] = useState(false)

  const planPref = normalizePhaseTierPref(config?.plan_tier)
  const actPref = normalizePhaseTierPref(config?.act_tier)
  const modelList = models ?? []

  const handlePick = async (phase: 'plan' | 'act', pref: PhaseTierPref) => {
    const current = phase === 'plan' ? planPref : actPref
    if (current === pref) return
    setBusy(true)
    try {
      await api.configure({ key: phase === 'plan' ? 'plan_tier' : 'act_tier', value: phaseTierConfigValue(pref) })
    } catch (e) {
      toastError(t('settings.models.phaseTier.failed'), e)
    } finally {
      // Always re-read the persisted config: success confirms the new
      // value, a failed write snaps the selection back to what is on disk.
      await refreshConfig().catch(() => {})
      setBusy(false)
    }
  }

  return (
    <section
      className="bg-surface-container-lowest border border-outline-variant/30 rounded-xl p-lg shadow-e1"
      aria-label={t('settings.models.phaseTier.title')}
      data-testid="phase-tier-section"
    >
      <h3 className="font-headline-md text-on-surface mb-xs">{t('settings.models.phaseTier.title')}</h3>
      <p className="text-body-sm text-on-surface-variant mb-md">{t('settings.models.phaseTier.subtitle')}</p>
      <div className="flex gap-lg flex-wrap">
        <PhaseSelect
          phase="plan"
          label={t('settings.models.phaseTier.plan')}
          value={planPref}
          models={modelList}
          disabled={busy}
          onPick={(phase, pref) => { void handlePick(phase, pref) }}
          t={t}
        />
        <PhaseSelect
          phase="act"
          label={t('settings.models.phaseTier.act')}
          value={actPref}
          models={modelList}
          disabled={busy}
          onPick={(phase, pref) => { void handlePick(phase, pref) }}
          t={t}
        />
      </div>
      <p className="mt-md text-label-sm text-on-surface-variant flex items-start gap-xs">
        <span className="material-symbols-outlined icon-sm" aria-hidden="true">info</span>
        {t('settings.models.phaseTier.precedence')}
      </p>
    </section>
  )
}
