import { useEffect, useState } from 'react'
import { useIntl, type PrimitiveType } from 'react-intl'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Spinner } from '@/components/ui/loading-state'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import * as api from '@/lib/tauri-api'
import type { ProviderConnection, ProviderKeySummary } from '@/types'
import { toastError } from '@/lib/errorToast'
import { cn } from '@/lib/utils'
import {
  canRemoveKey,
  keyHintAt,
  removalPromotesNext,
  validateNewKey,
} from '@/lib/providerKeys'

/**
 * R4-3 (desktop slice) — the per-provider "API keys" management panel
 * (Settings → Models). Opened from a provider card's key affordance and
 * rendered INLINE below the provider roster (deliberately not a
 * popup/dialog primitive — the same jsdom-friendly posture the Profiles
 * section's create/rename forms take).
 *
 * Backend contract (commands_keys.rs): rows are the credential store's
 * rotation order — index 0 is the ACTIVE key — and `masked_hint` never
 * carries full key material. Add takes plaintext (the same trust level as
 * the Add/Edit provider modal); remove on the ACTIVE key promotes the next
 * stored one, and the LAST remaining key is refused (replace it via the
 * provider's Edit modal instead — the panel says so and disables the
 * button). Activating hot-reloads the running client when the provider is
 * active, so the swap applies on the next send.
 */

interface ProviderKeysPanelProps {
  conn: ProviderConnection
  /** Close the panel (Clear the target in the parent). */
  onClose: () => void
  /** A mutation committed backend-side — the parent refreshes the roster
   *  (key-set presence) and provider status. */
  onKeysChanged?: () => Promise<void> | void
}

export function ProviderKeysPanel({ conn, onClose, onKeysChanged }: ProviderKeysPanelProps) {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, PrimitiveType>) => intl.formatMessage({ id }, values)
  const [rows, setRows] = useState<ProviderKeySummary[]>([])
  const [loading, setLoading] = useState(true)
  const [loadFailed, setLoadFailed] = useState(false)
  const [keyDraft, setKeyDraft] = useState('')
  const [keyError, setKeyError] = useState<string | null>(null)
  const [addBusy, setAddBusy] = useState(false)
  const [busyIndex, setBusyIndex] = useState<number | null>(null)
  const [removing, setRemoving] = useState<ProviderKeySummary | null>(null)
  const [removeBusy, setRemoveBusy] = useState(false)

  useEffect(() => {
    let cancelled = false
    api.listProviderKeys(conn.id)
      .then((r) => { if (!cancelled) setRows(r) })
      .catch((e) => {
        if (!cancelled) {
          setLoadFailed(true)
          console.warn('listProviderKeys error:', e)
        }
      })
      .finally(() => { if (!cancelled) setLoading(false) })
    return () => { cancelled = true }
  }, [conn.id])

  const submitAdd = async () => {
    const invalid = validateNewKey(keyDraft)
    if (invalid) {
      setKeyError(invalid)
      return
    }
    setAddBusy(true)
    try {
      const fresh = await api.addProviderKey(conn.id, keyDraft)
      setRows(fresh)
      setKeyDraft('')
      setKeyError(null)
      toast.success(
        t('settings.models.keys.added', { hint: fresh[fresh.length - 1]?.masked_hint ?? '' }),
      )
      await onKeysChanged?.()
    } catch (e) {
      // Backend errors (duplicate of a stored key, store failures) surface
      // via toast — the UI never sees full keys, so duplicates cannot be
      // pre-checked client-side.
      toastError(t('settings.models.keys.addFailed'), e)
    } finally {
      setAddBusy(false)
    }
  }

  const handleActivate = async (row: ProviderKeySummary) => {
    if (row.active || busyIndex != null) return
    setBusyIndex(row.index)
    try {
      const fresh = await api.activateProviderKey(conn.id, row.index)
      setRows(fresh)
      toast.success(t('settings.models.keys.activated', { hint: row.masked_hint }))
      await onKeysChanged?.()
    } catch (e) {
      toastError(t('settings.models.keys.activateFailed'), e)
    } finally {
      setBusyIndex(null)
    }
  }

  const confirmRemove = async () => {
    const row = removing
    if (!row) return
    setRemoveBusy(true)
    try {
      const fresh = await api.removeProviderKey(conn.id, row.index)
      setRows(fresh)
      setRemoving(null)
      toast.success(t('settings.models.keys.removed', { hint: row.masked_hint }))
      await onKeysChanged?.()
    } catch (e) {
      toastError(t('settings.models.keys.removeFailed'), e)
    } finally {
      setRemoveBusy(false)
    }
  }

  return (
    <section
      className="p-md rounded-xl border border-primary/40 bg-surface-container-low/60"
      aria-label={t('settings.models.keys.title')}
      data-testid="provider-keys-panel"
    >
      <div className="flex items-start justify-between gap-md mb-sm">
        <div>
          <h4 className="font-headline-sm text-on-surface flex items-center gap-xs">
            <span className="material-symbols-outlined icon-md text-primary" aria-hidden="true">key</span>
            {t('settings.models.keys.title')}
            <span className="font-label-md text-on-surface-variant">· {conn.display_name}</span>
          </h4>
          <p className="text-label-sm text-on-surface-variant">{t('settings.models.keys.subtitle')}</p>
        </div>
        <Button
          variant="ghost"
          onClick={onClose}
          aria-label={t('settings.models.keys.close')}
          data-testid="provider-keys-close"
          className="h-auto py-xs px-sm rounded-lg text-on-surface-variant hover:text-primary cursor-pointer shrink-0"
        >
          <span className="material-symbols-outlined icon-md" aria-hidden="true">close</span>
        </Button>
      </div>

      {/* Rotation note — the one-line contract: the engine walks down this
          list on auth failures / rate limits before any failover. */}
      <p className="text-label-sm text-on-surface-variant flex items-center gap-xs mb-md">
        <span className="material-symbols-outlined icon-sm" aria-hidden="true">autorenew</span>
        {t('settings.models.keys.rotationNote')}
      </p>

      {loading ? (
        <p className="text-body-sm text-on-surface-variant py-sm flex items-center gap-sm">
          <Spinner className="text-primary icon-sm" />
          {t('settings.models.keys.loading')}
        </p>
      ) : loadFailed ? (
        <p role="alert" className="text-body-sm text-error py-sm">
          {t('settings.models.keys.loadFailed')}
        </p>
      ) : rows.length === 0 ? (
        <p className="text-body-sm text-on-surface-variant py-sm" data-testid="provider-keys-empty">
          {t('settings.models.keys.empty')}
        </p>
      ) : (
        <ul className="space-y-xs mb-md" data-testid="provider-key-rows">
          {rows.map((row) => (
            <li
              key={row.index}
              className={cn(
                'flex items-center justify-between gap-md px-md py-sm rounded-lg border',
                row.active
                  ? 'border-primary/60 bg-primary-container/10'
                  : 'border-outline-variant/40',
              )}
              data-testid="provider-key-row"
              data-key-index={row.index}
            >
              <div className="flex items-center gap-sm min-w-0">
                <span
                  className={cn(
                    'material-symbols-outlined icon-sm shrink-0',
                    row.active ? 'text-primary' : 'text-on-surface-variant opacity-50',
                  )}
                  aria-hidden="true"
                >
                  {row.active ? 'star' : 'radio_button_unchecked'}
                </span>
                <span className="font-label-sm font-mono text-on-surface truncate" title={row.masked_hint}>
                  {row.masked_hint}
                </span>
                {row.active && (
                  <span className="px-xs py-[2px] bg-primary text-on-primary rounded-sm text-label-2xs font-bold uppercase tracking-wider shrink-0">
                    {t('settings.models.keys.activeBadge')}
                  </span>
                )}
              </div>
              <div className="flex items-center gap-xs shrink-0">
                {!row.active && (
                  <Button
                    variant="outline"
                    disabled={busyIndex != null || removeBusy || addBusy}
                    onClick={() => { void handleActivate(row) }}
                    aria-label={t('settings.models.keys.activateItem', { hint: row.masked_hint })}
                    data-testid={`provider-key-activate-${row.index}`}
                    className="h-auto py-xs px-md rounded-lg font-label-sm cursor-pointer"
                  >
                    {busyIndex === row.index ? <Spinner className="text-primary icon-sm" /> : t('settings.models.keys.activate')}
                  </Button>
                )}
                <Button
                  variant="ghost"
                  disabled={
                    busyIndex != null || removeBusy || addBusy || !canRemoveKey(rows, row.index)
                  }
                  onClick={() => setRemoving(row)}
                  aria-label={t('settings.models.keys.removeItem', { hint: row.masked_hint })}
                  data-testid={`provider-key-remove-${row.index}`}
                  title={!canRemoveKey(rows, row.index) ? t('settings.models.keys.lastKeyHint') : undefined}
                  className="h-auto py-xs px-sm rounded-lg text-on-surface-variant hover:text-error cursor-pointer disabled:opacity-50"
                >
                  <span className="material-symbols-outlined icon-sm" aria-hidden="true">delete</span>
                </Button>
              </div>
            </li>
          ))}
        </ul>
      )}
      {!loading && !loadFailed && rows.length === 1 && (
        <p className="text-label-sm text-on-surface-variant mb-md">
          {t('settings.models.keys.lastKeyHint')}
        </p>
      )}

      {/* Add form — plaintext, paste-friendly (the same trust level as the
          Add/Edit provider modal's key field). */}
      <form
        className="flex gap-sm"
        onSubmit={(e) => { e.preventDefault(); void submitAdd() }}
        data-testid="provider-keys-add-form"
      >
        <label htmlFor="provider-key-input" className="sr-only">
          {t('settings.models.keys.addAria')}
        </label>
        <input
          id="provider-key-input"
          data-testid="provider-key-input"
          type="password"
          autoComplete="off"
          spellCheck={false}
          className="flex-1 px-md py-sm bg-surface text-on-surface border border-outline-variant/50 rounded-lg outline-none focus:ring-2 focus:ring-primary font-body-sm font-mono"
          placeholder={t('settings.models.keys.addPlaceholder')}
          value={keyDraft}
          onChange={(e) => { setKeyDraft(e.target.value); setKeyError(null) }}
          aria-invalid={keyError != null}
          aria-describedby={keyError != null ? 'provider-key-error' : undefined}
        />
        <Button
          type="submit"
          disabled={addBusy || keyDraft.trim().length === 0}
          className="h-auto py-sm px-md rounded-lg font-label-md cursor-pointer shrink-0 flex items-center gap-xs"
        >
          {addBusy ? <Spinner className="text-primary icon-sm" /> : null}
          {t('settings.models.keys.add')}
        </Button>
      </form>
      {keyError && (
        <p id="provider-key-error" role="alert" className="mt-xs text-label-sm text-error">
          {t(keyError)}
        </p>
      )}

      {/* Remove confirmation. Removing the ACTIVE key promotes the next
          stored one — said out loud before the destructive call. */}
      <ConfirmDialog
        open={removing != null}
        destructive
        title={t('settings.models.keys.removeConfirmTitle')}
        message={
          removing && removalPromotesNext(rows, removing.index)
            ? t('settings.models.keys.removeActiveMessage', {
                hint: removing.masked_hint,
                next: keyHintAt(rows, 1) ?? '',
              })
            : t('settings.models.keys.removeConfirmMessage', { hint: removing?.masked_hint ?? '' })
        }
        confirmLabel={t('settings.models.keys.removeConfirmYes')}
        cancelLabel={t('settings.models.profiles.cancel')}
        busy={removeBusy}
        busyLabel={t('settings.models.keys.removing')}
        onConfirm={() => { void confirmRemove() }}
        onCancel={() => { if (!removeBusy) setRemoving(null) }}
      />
    </section>
  )
}
