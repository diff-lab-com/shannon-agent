import { useEffect, useState } from 'react'
import { useIntl } from 'react-intl'

import { useTauriEvent } from '@/hooks/useTauriEvent'
import { toastError } from '@/lib/errorToast'
import * as api from '@/lib/tauri-api'
import type { GatewayConfig, GatewayProcessState } from '@/types'

import { EngineConnectionCard } from './connections-settings/EngineConnectionCard'
import { GatewayProcessCard } from './connections-settings/GatewayProcessCard'
import { MobilePairingCard } from './connections-settings/MobilePairingCard'
import { PlatformsCard } from './connections-settings/PlatformsCard'
import { ALL_SLOTS, type Platform } from './connections-settings/types'

export default function ConnectionsSettings() {
  const intl = useIntl()
  const t = (id: string): string => intl.formatMessage({ id })

  const [config, setConfig] = useState<GatewayConfig | null>(null)
  const [hasSecret, setHasSecret] = useState<Record<string, boolean>>({})
  const [drafts, setDrafts] = useState<Record<string, string>>({})
  const [saving, setSaving] = useState<Platform | null>(null)
  const [engineDraft, setEngineDraft] = useState({ wsUrl: '', httpBaseUrl: '' })

  // E-1 方案 C — supervised gateway process state.
  const [procState, setProcState] = useState<GatewayProcessState | null>(null)

  // Pull the initial process snapshot once.
  useEffect(() => {
    api
      .gatewaySupervisorStatus()
      .then(setProcState)
      .catch((e) => toastError('gateway supervisor: status failed', e))
  }, [])

  // When the supervisor reports the child exited (crash, clean exit, or our own
  // stop), re-poll the status so the badge reflects the new state.
  useTauriEvent<{ reason: string; code: number | null }>('shannon:gateway-exited', () => {
    api.gatewaySupervisorStatus().then(setProcState).catch(() => {})
  })

  useEffect(() => {
    api
      .gatewayReadConfig()
      .then((cfg) => setConfig(cfg))
      .catch((e) => toastError('gateway config: load failed', e))
  }, [])

  // Seed the engine inputs once the config is in.
  useEffect(() => {
    if (config) {
      setEngineDraft({ wsUrl: config.engine.wsUrl, httpBaseUrl: config.engine.httpBaseUrl })
    }
  }, [config])

  // Probe each slot's keyring presence so the UI can show a badge without
  // ever pulling the secret value into the webview.
  useEffect(() => {
    let cancelled = false
    Promise.all(
      ALL_SLOTS.map(({ s }) =>
        api.gatewayHasSecret(s.key).then((present) => [s.key, present] as [string, boolean]),
      ),
    )
      .then((entries) => {
        if (!cancelled) setHasSecret(Object.fromEntries(entries))
      })
      .catch(() => {
        /* presence is best-effort; absence just shows "no credential" */
      })
    return () => {
      cancelled = true
    }
  }, [config])

  if (!config) {
    return (
      <div className="text-on-surface-variant font-body-sm animate-pulse">
        {t('settings.connections.title')}…
      </div>
    )
  }

  return (
    <div className="space-y-lg">
      <header className="space-y-1">
        <h1 className="font-display-md text-on-surface">{t('settings.connections.title')}</h1>
        <p className="text-on-surface-variant font-body-sm max-w-prose">
          {t('settings.connections.subtitle')}
        </p>
      </header>

      <EngineConnectionCard
        config={config}
        engineDraft={engineDraft}
        onEngineDraftChange={setEngineDraft}
        onConfigChange={setConfig}
      />

      <GatewayProcessCard procState={procState} onProcStateChange={setProcState} />

      <MobilePairingCard />

      <PlatformsCard
        config={config}
        hasSecret={hasSecret}
        drafts={drafts}
        saving={saving}
        onDraftChange={(key, value) => setDrafts((d) => ({ ...d, [key]: value }))}
        onSavedDrafts={(keys) =>
          setDrafts((d) => {
            const next = { ...d }
            keys.forEach((k) => delete next[k])
            return next
          })
        }
        onSavingChange={setSaving}
        onConfigChange={setConfig}
        onHasSecretChange={setHasSecret}
      />
    </div>
  )
}
