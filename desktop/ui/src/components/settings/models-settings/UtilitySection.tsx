import { useEffect, useState } from 'react'
import { useIntl, type PrimitiveType } from 'react-intl'
import * as api from '@/lib/tauri-api'
import { toastError } from '@/lib/errorToast'
import { UTILITY_SLOTS, utilitySlotValueOf, utilitySlotValueParts, type UtilitySlotId } from '@/lib/utilitySlots'

/**
 * S3-3 — Settings → Models "Utility slots": the two auxiliary model slots
 * (compaction, session summary) backed by providers.toml v2's
 * `auxiliary: HashMap<AuxRole, ActiveTarget>` — the schema slot that had zero
 * consumers until this chain. Each slot routes ONE kind of background task to
 * its own model (provider/base_url/credential re-resolved from the named
 * roster slot); an unset slot follows the global default, and a configured
 * slot whose provider has vanished falls back with a backend warning
 * (rendered here as the "no longer resolves" line).
 *
 * Orthogonality (review裁定⑦, mirrored from `desktop/src/utility_tier.rs`):
 * utility slots serve the background-task channel ONLY. They are NOT part of
 * the interactive precedence chain — a session model override beats the
 * phase tier beats the global default for the chat itself, and none of those
 * layers can see or change these slots. The section copy says exactly that.
 *
 * Selection state follows the persisted config (a failed write snaps back via
 * the re-read — the PhaseTierSection contract). Native `<select>` with
 * `<optgroup>` per roster provider — jsdom-stable for tests.
 */

/** Option value encoding "follow the global default". */
const FOLLOW_DEFAULT = ''

function SlotSelect({
  slotId,
  label,
  status,
  roster,
  disabled,
  onPick,
  t,
}: {
  slotId: UtilitySlotId
  label: string
  status: api.UtilitySlotStatus | undefined
  roster: api.UtilityRosterEntry[]
  disabled: boolean
  onPick: (slot: UtilitySlotId, value: string) => void
  t: (id: string, values?: Record<string, PrimitiveType>) => string
}) {
  // The persisted selection re-encoded as the option value ('' when unset).
  const value =
    status?.provider && status?.model ? `${status.provider}::${status.model}` : FOLLOW_DEFAULT
  const selectId = `utility-slot-select-${slotId}`
  const configured = value !== FOLLOW_DEFAULT
  return (
    <div className="flex-1 min-w-[220px]">
      <label htmlFor={selectId} className="font-label-md text-on-surface font-bold block mb-xs">
        {label}
      </label>
      <select
        id={selectId}
        data-testid={`utility-slot-select-${slotId}`}
        disabled={disabled}
        className="w-full px-md py-sm bg-surface text-on-surface border border-outline-variant/50 rounded-lg outline-none focus:ring-2 focus:ring-primary font-body-sm cursor-pointer disabled:opacity-60"
        value={value}
        onChange={(e) => onPick(slotId, e.target.value)}
      >
        <option value={FOLLOW_DEFAULT}>{t('settings.models.utility.followDefault')}</option>
        {roster
          .filter((r) => r.models.length > 0)
          .map((r) => (
            <optgroup key={r.provider_id} label={r.display_name}>
              {r.models.map((m) => (
                <option key={`${r.provider_id}::${m}`} value={`${r.provider_id}::${m}`}>
                  {m}
                </option>
              ))}
            </optgroup>
          ))}
      </select>
      <p
        className="mt-xs text-label-sm text-on-surface-variant"
        data-testid={`utility-slot-resolved-${slotId}`}
      >
        {!configured
          ? t('settings.models.utility.followHint')
          : status?.resolves === false
            ? t('settings.models.utility.unresolved')
            : t('settings.models.utility.resolved', {
                model: status?.model ?? '',
                provider: status?.provider ?? '',
              })}
      </p>
    </div>
  )
}

export function UtilitySection() {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, PrimitiveType>) => intl.formatMessage({ id }, values)
  const [view, setView] = useState<api.UtilitySlotsView | null>(null)
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    let cancelled = false
    api
      .getUtilitySlots()
      .then((v) => {
        if (!cancelled) setView(v)
      })
      .catch((e) => console.warn('getUtilitySlots error:', e))
    return () => {
      cancelled = true
    }
  }, [])

  const handlePick = async (slot: UtilitySlotId, value: string) => {
    if (!view) return
    const def = UTILITY_SLOTS.find((d) => d.id === slot)
    if (!def) return
    const current = utilitySlotValueOf(view.slots.find((s) => s.role === def.role))
    if (current === value) return
    setBusy(true)
    try {
      const parts = utilitySlotValueParts(value)
      if (parts) {
        await api.setUtilitySlot(def.role, parts.provider, parts.model)
      } else {
        await api.setUtilitySlot(def.role, null, null)
      }
    } catch (e) {
      toastError(t('settings.models.utility.failed'), e)
    } finally {
      // Always re-read the persisted state: success confirms the new value,
      // a failed write snaps the selection back to what is on disk (the
      // PhaseTierSection contract).
      await api
        .getUtilitySlots()
        .then((v) => setView(v))
        .catch(() => {})
      setBusy(false)
    }
  }

  return (
    <section
      className="bg-surface-container-lowest border border-outline-variant/30 rounded-xl p-lg shadow-e1"
      aria-label={t('settings.models.utility.title')}
      data-testid="utility-slots-section"
    >
      <h3 className="font-headline-md text-on-surface mb-xs">{t('settings.models.utility.title')}</h3>
      <p className="text-body-sm text-on-surface-variant mb-md">{t('settings.models.utility.subtitle')}</p>
      <div className="flex gap-lg flex-wrap">
        {UTILITY_SLOTS.map((slot) => (
          <SlotSelect
            key={slot.id}
            slotId={slot.id}
            label={t(slot.labelKey)}
            status={view?.slots.find((s) => s.role === slot.role)}
            roster={view?.roster ?? []}
            disabled={busy || view == null}
            onPick={(id, value) => {
              void handlePick(id, value)
            }}
            t={t}
          />
        ))}
      </div>
      <p className="mt-md text-label-sm text-on-surface-variant flex items-start gap-xs">
        <span className="material-symbols-outlined icon-sm" aria-hidden="true">info</span>
        {t('settings.models.utility.orthogonal')}
      </p>
    </section>
  )
}
