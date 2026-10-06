import { useState } from 'react'
import { Spinner } from '@/components/ui/loading-state'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import AddProviderModal from '@/components/settings/AddProviderModal'
import * as api from '@/lib/tauri-api'
import { toastError } from '@/lib/errorToast'
import { useSessions } from '@/context/SessionContext'
import type { ProviderConnection, ProvidersFile } from '@/types'
import { KIND_INFO } from './types'
import { toastTestResult } from './utils'
import { TestAllResultsPanel } from './TestAllResultsPanel'
import { ProviderCard } from './ProviderCard'
import { ProviderKeysPanel } from './ProviderKeysPanel'
import { RecommendedFallbackPanel } from './RecommendedFallbackPanel'

export function ProvidersSection({
  providersFile,
  loading,
  onChange,
  onActivated,
}: {
  providersFile: ProvidersFile
  loading: boolean
  onChange: (f: ProvidersFile) => void
  onActivated: () => Promise<void>
}) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const [modalOpen, setModalOpen] = useState(false)
  const [editing, setEditing] = useState<ProviderConnection | null>(null)
  const [testingId, setTestingId] = useState<string | null>(null)
  const [activatingId, setActivatingId] = useState<string | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<ProviderConnection | null>(null)
  const [testAllRunning, setTestAllRunning] = useState(false)
  const [testAllRows, setTestAllRows] = useState<api.ProviderTestRow[] | null>(null)
  // R4-3 (desktop slice): the provider whose "API keys" panel is open —
  // rendered INLINE below the roster (no popup primitives).
  const [keysTarget, setKeysTarget] = useState<ProviderConnection | null>(null)
  // S3-4: the provider whose "recommended fallback chain" panel is open —
  // same inline pattern. The panel is candidates-only until the user
  // confirms the apply inside it (failover is never enabled automatically).
  const [fallbackTarget, setFallbackTarget] = useState<ProviderConnection | null>(null)
  // P0-4: pending activation gated behind the prompt-cache bust warning
  // (only shown when the current chat session already has usage).
  const { currentSessionId } = useSessions()
  const [cacheBustTarget, setCacheBustTarget] = useState<ProviderConnection | null>(null)

  const handleTest = async (conn: ProviderConnection) => {
    // Only the active provider's key is mirrored into config; for a connection
    // we can only test when a key is set on it. Ollama needs no key.
    const info = KIND_INFO[conn.kind]
    if (info?.needsKey && !conn.has_api_key) {
      toast.error(intl.formatMessage({ id: 'settings.models.providers.needKey' }, { label: conn.display_name }))
      return
    }
    setTestingId(conn.id)
    try {
      // P0-7: the wire type no longer carries the raw api_key — only
      // `has_api_key: bool`. `testProviderCredentials` passes `apiKey: null`
      // + the connection id, so the backend resolves the STORED credential
      // (same fallback "Test all" uses) instead of probing with an empty key
      // and misreporting every saved provider as "Invalid key".
      const result = await api.testProviderCredentials(conn.kind, conn.base_url ?? null, null, conn.id)
      toastTestResult(intl, result, conn.kind)
    } catch (e) {
      toastError(t('settings.models.testResult.failed'), e)
    } finally {
      setTestingId(null)
    }
  }

  const confirmActivate = async (conn: ProviderConnection) => {
    setCacheBustTarget(null)
    setActivatingId(conn.id)
    try {
      await api.setActiveProvider(conn.id)
      // Re-fetch to pick up the masked file the backend persisted.
      const fresh = await api.listProviders()
      onChange(fresh)
      await onActivated()
      toast.success(intl.formatMessage({ id: 'settings.models.providers.activated' }, { label: conn.display_name }))
    } catch (e) {
      toastError(t('settings.models.providers.activateFailed'), e)
    } finally {
      setActivatingId(null)
    }
  }

  // P0-4: switching the provider busts the session's prompt cache. When the
  // current chat session already has usage, ask before invalidating it.
  const handleActivate = async (conn: ProviderConnection) => {
    if (currentSessionId) {
      try {
        const summary = await api.getSessionUsage(currentSessionId)
        if (summary.events > 0) {
          setCacheBustTarget(conn)
          return
        }
      } catch {
        // Usage read failed — proceed with the switch rather than blocking it.
      }
    }
    await confirmActivate(conn)
  }

  // Delete confirmation flows through the ConfirmDialog (state-driven) instead
  // of a native window.confirm, so it matches the app's design system and locale.
  const confirmDeleteProvider = async () => {
    const conn = deleteTarget
    setDeleteTarget(null)
    if (!conn) return
    try {
      const fresh = await api.deleteProvider(conn.id)
      onChange(fresh)
      toast.success(intl.formatMessage({ id: 'settings.models.providers.deleted' }, { label: conn.display_name }))
    } catch (e) {
      toastError(t('settings.models.providers.deleteFailed'), e)
    }
  }

  const handleSaved = (fresh: ProvidersFile) => {
    onChange(fresh)
    setModalOpen(false)
    setEditing(null)
    // Stale "test all" results would mislead if the user then re-runs the
    // batch probe — drop them and force the user to re-run.
    setTestAllRows(null)
    toast.success(t('settings.models.providers.saved'))
  }

  // R4-3: a key mutation committed backend-side (add/activate/remove) can
  // change the provider's key-set presence (first key added) — refresh the
  // roster + status/catalog through the same paths activation uses.
  const handleKeysChanged = async () => {
    try {
      onChange(await api.listProviders())
    } catch (e) {
      console.warn('listProviders after key mutation failed:', e)
    }
    await onActivated()
  }

  // S3-4: a fallback chain was committed backend-side — refresh the roster
  // (the connection's `fallback_models` changed) the same way key
  // mutations do. No status/catalog refresh needed: the chain only affects
  // failover resolution (rebuild happens backend-side).
  const handleFallbackApplied = async () => {
    try {
      onChange(await api.listProviders())
    } catch (e) {
      console.warn('listProviders after fallback write failed:', e)
    }
    setFallbackTarget(null)
  }

  const handleTestAll = async () => {
    if (testAllRunning) return
    setTestAllRunning(true)
    try {
      const rows = await api.testAllProviders()
      setTestAllRows(rows)
    } catch (e) {
      toastError(t('settings.models.testResult.failed'), e)
    } finally {
      setTestAllRunning(false)
    }
  }

  return (
    <section className="bg-surface-container-lowest border border-outline-variant/30 rounded-xl p-lg shadow-e1">
      <div className="flex items-center justify-between mb-md">
        <div>
          <h3 className="font-headline-md text-on-surface">{t('settings.models.providers.title')}</h3>
          <p className="text-body-sm text-on-surface-variant">{t('settings.models.providers.subtitle')}</p>
        </div>
        <Button
          className="px-md py-sm bg-primary text-on-primary font-label-md rounded-lg hover:bg-primary/90 transition-colors flex items-center gap-sm whitespace-nowrap cursor-pointer"
          onClick={() => { setEditing(null); setModalOpen(true) }}
        >
          <span className="material-symbols-outlined icon-md">add</span>
          {t('settings.models.providers.add')}
        </Button>
      </div>
      <div className="flex justify-end -mt-md mb-md">
        <Button
          variant="ghost"
          className="px-md py-sm text-on-surface-variant hover:text-primary whitespace-nowrap cursor-pointer disabled:opacity-50"
          onClick={handleTestAll}
          disabled={testAllRunning || providersFile.providers.length === 0}
          aria-label={t('settings.models.providers.testAll')}
        >
          {testAllRunning ? (
            <>
              <Spinner className="text-body-lg" />
              {t('settings.models.providers.testAllInProgress')}
            </>
          ) : (
            <>
              <span className="material-symbols-outlined icon-md">cable</span>
              {t('settings.models.providers.testAll')}
            </>
          )}
        </Button>
      </div>

      {loading ? (
        <p className="text-body-sm text-on-surface-variant py-lg text-center">{t('settings.models.providers.loading')}</p>
      ) : providersFile.providers.length === 0 ? (
        <p className="text-body-sm text-on-surface-variant py-lg text-center">{t('settings.models.providers.empty')}</p>
      ) : (
        <div className="grid grid-cols-1 gap-sm">
          {providersFile.providers.map(conn => (
            <ProviderCard
              key={conn.id}
              conn={conn}
              isActive={providersFile.active_provider_id === conn.id}
              testingId={testingId}
              activatingId={activatingId}
              intl={intl}
              t={t}
              onTest={() => handleTest(conn)}
              onActivate={() => handleActivate(conn)}
              onEdit={() => { setEditing(conn); setModalOpen(true) }}
              onDelete={() => setDeleteTarget(conn)}
              onKeys={() => setKeysTarget(keysTarget?.id === conn.id ? null : conn)}
              keysOpen={keysTarget?.id === conn.id}
              onFallback={() => setFallbackTarget(fallbackTarget?.id === conn.id ? null : conn)}
              fallbackOpen={fallbackTarget?.id === conn.id}
            />
          ))}
        </div>
      )}

      {/* R4-3 (desktop slice): inline per-provider API-keys management —
          list (active marker + masked hints), add, activate, remove. */}
      {keysTarget ? (
        <ProviderKeysPanel
          conn={keysTarget}
          onClose={() => setKeysTarget(null)}
          onKeysChanged={handleKeysChanged}
        />
      ) : null}

      {/* S3-4: inline recommended-fallback-chain panel — candidates only
          until the user confirms the apply inside it. Also reachable from
          the Test-all panel's failed (rate-limited / quota-exhausted) rows,
          where a fallback chain is exactly the cure. */}
      {fallbackTarget ? (
        <RecommendedFallbackPanel
          conn={fallbackTarget}
          onClose={() => setFallbackTarget(null)}
          onApplied={handleFallbackApplied}
        />
      ) : null}

      {testAllRows !== null ? (
        testAllRows.length === 0 ? (
          <p className="text-body-sm text-on-surface-variant py-md text-center">{t('settings.models.providers.testAllEmpty')}</p>
        ) : (
          <TestAllResultsPanel
            rows={testAllRows}
            intl={intl}
            t={t}
            onSuggestFallback={(providerId) => {
              const conn = providersFile.providers.find((p) => p.id === providerId)
              if (conn) setFallbackTarget(conn)
            }}
          />
        )
      ) : null}

      {modalOpen ? (
        <AddProviderModal
          editing={editing}
          onClose={() => { setModalOpen(false); setEditing(null) }}
          onSaved={handleSaved}
        />
      ) : null}

      {cacheBustTarget ? (
        <ConfirmDialog
          open
          title={t('settings.models.providers.cacheWarning.title')}
          message={t('settings.models.providers.cacheWarning.message')}
          confirmLabel={t('settings.models.providers.cacheWarning.confirm')}
          cancelLabel={t('settings.models.providers.cacheWarning.cancel')}
          onConfirm={() => void confirmActivate(cacheBustTarget)}
          onCancel={() => setCacheBustTarget(null)}
        />
      ) : null}

      {deleteTarget ? (
        <ConfirmDialog
          open
          destructive
          title={t('settings.models.providers.deleteConfirmTitle')}
          message={intl.formatMessage({ id: 'settings.models.providers.confirmDelete' }, { label: deleteTarget.display_name })}
          confirmLabel={t('settings.models.providers.delete')}
          cancelLabel={t('settings.models.providers.cancel')}
          onConfirm={confirmDeleteProvider}
          onCancel={() => setDeleteTarget(null)}
        />
      ) : null}
    </section>
  )
}