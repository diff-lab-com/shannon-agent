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
  isDuplicateRename,
  isLastRemainingProfile,
  needsEmptyConfirm,
  profileFallbackAfterDelete,
  validateProfileName,
} from '@/lib/providerProfiles'

/**
 * R3-2 (desktop slice) — Settings → Models "Profiles": the engine store's
 * named model profiles (providers.toml v2 `profiles` map + the
 * `active_profile` pointer). R5 completes the approved scope: list (name,
 * provider count, active marker), switch (confirm when the target is
 * empty), create (name prompt), rename (inline edit form) and delete
 * (ConfirmDialog that names the engine's active-pointer fallback when the
 * target is the ACTIVE profile; `force` semantics are automatic in the
 * command).
 *
 * Switching/renaming-the-active/deleting-the-active refresh the provider
 * status + catalog through the SAME paths the R2-2 refresh and provider
 * activation use (`refreshModels` + `refreshStatus` via `onSwitched`); the
 * backend re-points the global default by rebuilding the client config.
 */

interface ProfilesSectionProps {
  /** Called after a successful op that moved the global default so the parent refreshes status + catalog. */
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
  // R5: inline rename form (non-popup — the create form's pattern), one at
  // a time. `renaming.name` is the profile being renamed; `draft` mirrors
  // the input.
  const [renaming, setRenaming] = useState<{ name: string; draft: string } | null>(null)
  const [renameError, setRenameError] = useState<string | null>(null)
  const [renameBusy, setRenameBusy] = useState(false)
  // R5: pending delete — confirmed through the jsdom-safe ConfirmDialog.
  const [deleting, setDeleting] = useState<ProviderProfileSummary | null>(null)
  const [deleteBusy, setDeleteBusy] = useState(false)
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

  // ── R5: rename (inline form, same validation contract as create) ────────

  const startRename = (row: ProviderProfileSummary) => {
    setRenaming({ name: row.name, draft: row.name })
    setRenameError(null)
    setCreating(false)
  }

  const submitRename = async () => {
    if (!renaming) return
    const { name: current, draft } = renaming
    const next = draft.trim()
    // Unchanged (or re-cased to itself) → close without a round trip; the
    // backend treats old == new as a no-op, and this keeps the success
    // toast honest.
    if (next === current) {
      setRenaming(null)
      return
    }
    const invalid = validateProfileName(next)
    if (invalid) {
      setRenameError(invalid)
      return
    }
    if (isDuplicateRename(rows, current, next)) {
      setRenameError('settings.models.profiles.nameDuplicate')
      return
    }
    setRenameBusy(true)
    try {
      const fresh = await api.renameProviderProfile(current, next)
      setRows(fresh)
      setRenaming(null)
      toast.success(t('settings.models.profiles.renamed', { old: current, name: next }))
      // Renaming the ACTIVE profile re-points the global default's label —
      // refresh status + catalog like a switch would.
      if (rows.find((r) => r.name === current)?.active) await onSwitched?.()
    } catch (e) {
      // Engine errors (duplicate, unknown source) surface inline + toast.
      toastError(t('settings.models.profiles.renameFailed'), e)
    } finally {
      setRenameBusy(false)
    }
  }

  // ── R5: delete (ConfirmDialog; the fallback warning is computed client- ─
  //    side from the same rule the engine applies) ────────────────────────

  const confirmDelete = async () => {
    const row = deleting
    if (!row) return
    setDeleteBusy(true)
    try {
      const outcome = await api.deleteProviderProfile(row.name, true)
      setRows(outcome.profiles)
      setDeleting(null)
      toast.success(t('settings.models.profiles.deleted', { name: row.name }))
      // Deleting the ACTIVE profile moved the engine's active pointer —
      // refresh status + catalog (same paths as a switch).
      if (outcome.became_active) await onSwitched?.()
    } catch (e) {
      toastError(t('settings.models.profiles.deleteFailed'), e)
    } finally {
      setDeleteBusy(false)
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

      {renaming && (
        <form
          className="mb-md p-md rounded-xl border border-outline-variant/40 bg-surface-container-low/40"
          onSubmit={(e) => { e.preventDefault(); void submitRename() }}
          data-testid="profile-rename-form"
        >
          <label htmlFor="profile-rename-input" className="font-label-md text-on-surface font-bold block mb-xs">
            {t('settings.models.profiles.renameTitle', { name: renaming.name })}
          </label>
          <div className="flex gap-sm">
            <input
              id="profile-rename-input"
              data-testid="profile-rename-input"
              autoFocus
              className="flex-1 px-md py-sm bg-surface text-on-surface border border-outline-variant/50 rounded-lg outline-none focus:ring-2 focus:ring-primary font-body-sm"
              placeholder={t('settings.models.profiles.namePlaceholder')}
              value={renaming.draft}
              onChange={(e) => { setRenaming({ ...renaming, draft: e.target.value }); setRenameError(null) }}
              aria-invalid={renameError != null}
              aria-describedby={renameError != null ? 'profile-rename-error' : undefined}
            />
            <Button
              type="submit"
              disabled={renameBusy || renaming.draft.trim().length === 0}
              className="h-auto py-sm px-md rounded-lg font-label-md cursor-pointer shrink-0"
            >
              {renameBusy ? <Spinner className="text-primary icon-sm" /> : t('settings.models.profiles.confirmRename')}
            </Button>
            <Button
              type="button"
              variant="ghost"
              disabled={renameBusy}
              onClick={() => setRenaming(null)}
              className="h-auto py-sm px-md rounded-lg font-label-md cursor-pointer shrink-0 text-on-surface-variant"
            >
              {t('settings.models.profiles.cancel')}
            </Button>
          </div>
          {renameError && (
            <p id="profile-rename-error" role="alert" className="mt-xs text-label-sm text-error">
              {t(renameError)}
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
              {!renaming && !switching && (
                <div className="flex items-center gap-xs shrink-0">
                  {!row.active && (
                    <Button
                      variant="outline"
                      disabled={switching != null}
                      onClick={() => { void handleSwitch(row) }}
                      aria-label={t('settings.models.profiles.switchTo', { name: row.name })}
                      data-testid={`profile-switch-${row.name}`}
                      className="h-auto py-sm px-md rounded-lg font-label-md cursor-pointer flex items-center gap-xs"
                    >
                      {switching === row.name ? <Spinner className="text-primary icon-sm" /> : null}
                      {t('settings.models.profiles.switchTo', { name: row.name })}
                    </Button>
                  )}
                  <Button
                    variant="ghost"
                    disabled={renameBusy || deleteBusy || creating}
                    onClick={() => startRename(row)}
                    aria-label={t('settings.models.profiles.renameItem', { name: row.name })}
                    data-testid={`profile-rename-${row.name}`}
                    className="h-auto py-sm px-sm rounded-lg text-on-surface-variant hover:text-primary cursor-pointer"
                  >
                    <span className="material-symbols-outlined icon-md" aria-hidden="true">edit</span>
                  </Button>
                  <Button
                    variant="ghost"
                    disabled={renameBusy || deleteBusy || creating || isLastRemainingProfile(rows)}
                    onClick={() => setDeleting(row)}
                    aria-label={t('settings.models.profiles.deleteItem', { name: row.name })}
                    data-testid={`profile-delete-${row.name}`}
                    title={isLastRemainingProfile(rows) ? t('settings.models.profiles.lastProfile') : undefined}
                    className="h-auto py-sm px-sm rounded-lg text-on-surface-variant hover:text-error cursor-pointer disabled:opacity-50"
                  >
                    <span className="material-symbols-outlined icon-md" aria-hidden="true">delete</span>
                  </Button>
                </div>
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

      {/* R5: delete confirmation. Deleting the ACTIVE profile moves the
          engine's pointer — the message names the fallback profile the
          engine will pick (client mirror of `remove_model_profile`'s
          fallback rule), and `force` semantics are automatic backend-side. */}
      <ConfirmDialog
        open={deleting != null}
        destructive
        title={t('settings.models.profiles.deleteConfirmTitle')}
        message={
          deleting?.active
            ? t('settings.models.profiles.deleteActiveMessage', {
                name: deleting.name,
                fallback: profileFallbackAfterDelete(rows, deleting.name) ?? '',
              })
            : t('settings.models.profiles.deleteConfirmMessage', { name: deleting?.name ?? '' })
        }
        confirmLabel={t('settings.models.profiles.deleteConfirmYes')}
        cancelLabel={t('settings.models.profiles.cancel')}
        busy={deleteBusy}
        busyLabel={t('settings.models.profiles.deleting')}
        onConfirm={() => { void confirmDelete() }}
        onCancel={() => { if (!deleteBusy) setDeleting(null) }}
      />
    </section>
  )
}
