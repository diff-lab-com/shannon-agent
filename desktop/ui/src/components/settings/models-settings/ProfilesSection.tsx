import { useEffect, useState } from 'react'
import { useIntl, type PrimitiveType } from 'react-intl'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Spinner } from '@/components/ui/loading-state'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import * as api from '@/lib/tauri-api'
import type { ProviderProfileSummary } from '@/types'
import { toastError } from '@/lib/errorToast'
import { cn } from '@/lib/utils'
import {
  isDuplicateName,
  needsEmptyConfirm,
  validateProfileName,
} from '@/lib/providerProfiles'

/**
 * R3-2 (desktop slice) — Settings → Models "Profiles": the engine store's
 * named model profiles (providers.toml v2 `profiles` map + the
 * `active_profile` pointer). Approved scope: list (name, provider count,
 * active marker), switch (confirm when the target is empty), create (name
 * prompt). NO rename/delete UI this batch.
 *
 * Switching refreshes the provider status + catalog through the SAME paths
 * the R2-2 refresh and provider activation use (`refreshModels` +
 * `refreshStatus` via `onSwitched`); the backend re-points the global
 * default by rebuilding the client config.
 */

interface ProfilesSectionProps {
  /** Called after a successful switch so the parent refreshes status + catalog. */
  onSwitched?: () => Promise<void> | void
}

export function ProfilesSection({ onSwitched }: ProfilesSectionProps) {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, PrimitiveType>) => intl.formatMessage({ id }, values)
  const [rows, setRows] = useState<ProviderProfileSummary[]>([])
  const [loading, setLoading] = useState(true)
  const [switching, setSwitching] = useState<string | null>(null)
  const [creating, setCreating] = useState(false)
  const [nameDraft, setNameDraft] = useState('')
  const [nameError, setNameError] = useState<string | null>(null)
  const [createBusy, setCreateBusy] = useState(false)
  // Pending switch to an EMPTY profile — needs explicit confirmation
  // (the global default degrades to "no active model" until the profile
  // gets providers).
  const [pendingEmpty, setPendingEmpty] = useState<ProviderProfileSummary | null>(null)
  const [confirmBusy, setConfirmBusy] = useState(false)

  useEffect(() => {
    let cancelled = false
    api.listProviderProfiles()
      .then((r) => { if (!cancelled) setRows(r) })
      .catch((e) => console.warn('listProviderProfiles error:', e))
      .finally(() => { if (!cancelled) setLoading(false) })
    return () => { cancelled = true }
  }, [])

  const handleSwitch = async (row: ProviderProfileSummary) => {
    if (row.active || switching != null) return
    // Empty target → confirm first; the switch itself degrades the global
    // default to "no active model" until the profile gains providers.
    if (needsEmptyConfirm(row)) {
      setPendingEmpty(row)
      return
    }
    setSwitching(row.name)
    try {
      const fresh = await api.setActiveProviderProfile(row.name)
      setRows(fresh)
      await onSwitched?.()
      toast.success(t('settings.models.profiles.switched', { name: row.name }))
    } catch (e) {
      toastError(t('settings.models.profiles.switchFailed'), e)
    } finally {
      setSwitching(null)
    }
  }

  const confirmEmptySwitch = async () => {
    const row = pendingEmpty
    if (!row) return
    setConfirmBusy(true)
    try {
      const fresh = await api.setActiveProviderProfile(row.name)
      setRows(fresh)
      setPendingEmpty(null)
      await onSwitched?.()
      toast.success(t('settings.models.profiles.switched', { name: row.name }))
    } catch (e) {
      toastError(t('settings.models.profiles.switchFailed'), e)
    } finally {
      setConfirmBusy(false)
    }
  }

  const startCreate = () => {
    setCreating(true)
    setNameDraft('')
    setNameError(null)
  }

  const submitCreate = async () => {
    const invalid = validateProfileName(nameDraft)
    if (invalid) {
      setNameError(invalid)
      return
    }
    if (isDuplicateName(rows, nameDraft)) {
      setNameError('settings.models.profiles.nameDuplicate')
      return
    }
    setCreateBusy(true)
    try {
      const fresh = await api.createProviderProfile(nameDraft.trim())
      setRows(fresh)
      setCreating(false)
      toast.success(t('settings.models.profiles.created', { name: nameDraft.trim() }))
    } catch (e) {
      toastError(t('settings.models.profiles.createFailed'), e)
    } finally {
      setCreateBusy(false)
    }
  }

  return (
    <section
      className="bg-surface-container-lowest border border-outline-variant/30 rounded-xl p-lg shadow-e1"
      aria-label={t('settings.models.profiles.title')}
      data-testid="profiles-section"
    >
      <div className="flex items-start justify-between gap-md mb-md">
        <div>
          <h3 className="font-headline-md text-on-surface">{t('settings.models.profiles.title')}</h3>
          <p className="text-body-sm text-on-surface-variant">{t('settings.models.profiles.subtitle')}</p>
        </div>
        {!creating && (
          <Button
            variant="outline"
            onClick={startCreate}
            aria-label={t('settings.models.profiles.create')}
            data-testid="profile-create"
            className="h-auto py-sm px-md rounded-lg font-label-md flex items-center gap-xs shrink-0 cursor-pointer"
          >
            <span className="material-symbols-outlined icon-sm" aria-hidden="true">add</span>
            {t('settings.models.profiles.create')}
          </Button>
        )}
      </div>

      {creating && (
        <form
          className="mb-md p-md rounded-xl border border-outline-variant/40 bg-surface-container-low/40"
          onSubmit={(e) => { e.preventDefault(); void submitCreate() }}
        >
          <label htmlFor="profile-name-input" className="font-label-md text-on-surface font-bold block mb-xs">
            {t('settings.models.profiles.nameAria')}
          </label>
          <div className="flex gap-sm">
            <input
              id="profile-name-input"
              data-testid="profile-name-input"
              autoFocus
              className="flex-1 px-md py-sm bg-surface text-on-surface border border-outline-variant/50 rounded-lg outline-none focus:ring-2 focus:ring-primary font-body-sm"
              placeholder={t('settings.models.profiles.namePlaceholder')}
              value={nameDraft}
              onChange={(e) => { setNameDraft(e.target.value); setNameError(null) }}
              aria-invalid={nameError != null}
              aria-describedby={nameError != null ? 'profile-name-error' : undefined}
            />
            <Button
              type="submit"
              disabled={createBusy || nameDraft.trim().length === 0}
              className="h-auto py-sm px-md rounded-lg font-label-md cursor-pointer shrink-0"
            >
              {createBusy ? <Spinner className="text-primary icon-sm" /> : t('settings.models.profiles.confirmCreate')}
            </Button>
            <Button
              type="button"
              variant="ghost"
              disabled={createBusy}
              onClick={() => setCreating(false)}
              className="h-auto py-sm px-md rounded-lg font-label-md cursor-pointer shrink-0 text-on-surface-variant"
            >
              {t('settings.models.profiles.cancel')}
            </Button>
          </div>
          {nameError && (
            <p id="profile-name-error" role="alert" className="mt-xs text-label-sm text-error">
              {t(nameError)}
            </p>
          )}
        </form>
      )}

      {loading ? (
        <p className="text-body-sm text-on-surface-variant py-md flex items-center gap-sm">
          <Spinner className="text-primary icon-sm" />
          {t('settings.models.profiles.loading')}
        </p>
      ) : rows.length === 0 ? (
        <p className="text-body-sm text-on-surface-variant py-md">{t('settings.models.profiles.empty')}</p>
      ) : (
        <ul className="space-y-sm" data-testid="profile-rows">
          {rows.map((row) => (
            <li
              key={row.name}
              className={cn(
                'flex items-center justify-between gap-md p-md rounded-xl border transition-all',
                row.active
                  ? 'border-2 border-primary bg-primary-container/5'
                  : 'border-outline-variant/50 hover:border-primary/40',
              )}
              data-testid="profile-row"
              data-profile={row.name}
            >
              <div className="min-w-0">
                <div className="flex items-center gap-xs flex-wrap">
                  <span className={cn('font-headline-sm', row.active ? 'text-link font-bold' : 'text-on-surface')}>
                    {row.name}
                  </span>
                  {row.active && (
                    <span className="px-xs py-[2px] bg-primary text-on-primary rounded-sm text-label-2xs font-bold uppercase tracking-wider">
                      {t('settings.models.profiles.activeBadge')}
                    </span>
                  )}
                </div>
                <p className="text-label-sm text-on-surface-variant">
                  {intl.formatMessage({ id: 'settings.models.profiles.providerCount' }, { count: row.provider_count })}
                  {' · '}
                  {row.model
                    ? intl.formatMessage({ id: 'settings.models.profiles.modelLabel' }, { model: row.model })
                    : t('settings.models.profiles.noModel')}
                </p>
              </div>
              {!row.active && (
                <Button
                  variant="outline"
                  disabled={switching != null}
                  onClick={() => { void handleSwitch(row) }}
                  aria-label={t('settings.models.profiles.switchTo', { name: row.name })}
                  data-testid={`profile-switch-${row.name}`}
                  className="h-auto py-sm px-md rounded-lg font-label-md shrink-0 cursor-pointer flex items-center gap-xs"
                >
                  {switching === row.name ? <Spinner className="text-primary icon-sm" /> : null}
                  {t('settings.models.profiles.switchTo', { name: row.name })}
                </Button>
              )}
            </li>
          ))}
        </ul>
      )}

      <ConfirmDialog
        open={pendingEmpty != null}
        title={t('settings.models.profiles.emptyConfirmTitle')}
        message={t('settings.models.profiles.emptyConfirmMessage', { name: pendingEmpty?.name ?? '' })}
        confirmLabel={t('settings.models.profiles.emptyConfirmYes')}
        cancelLabel={t('settings.models.profiles.cancel')}
        busy={confirmBusy}
        busyLabel={t('settings.models.profiles.switching')}
        onConfirm={() => { void confirmEmptySwitch() }}
        onCancel={() => { if (!confirmBusy) setPendingEmpty(null) }}
      />
    </section>
  )
}
