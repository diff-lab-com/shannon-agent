import { useEffect, useState } from 'react'
import { useIntl } from 'react-intl'
import { useNavigate } from 'react-router-dom'

import { useTauriEvent } from '@/hooks/useTauriEvent'
import { toastError } from '@/lib/errorToast'
import * as api from '@/lib/tauri-api'
import type { GatewayConfig, GatewayProcessState } from '@/types'

import { EngineConnectionCard } from './connections-settings/EngineConnectionCard'
import { GatewayProcessCard } from './connections-settings/GatewayProcessCard'
import { MobileDispatchCard } from './connections-settings/MobileDispatchCard'
import { PlatformsCard } from './connections-settings/PlatformsCard'
import { ALL_SLOTS, type Platform } from './connections-settings/types'

export default function ConnectionsSettings() {
  const intl = useIntl()
  const t = (id: string): string => intl.formatMessage({ id })
  // X3 互链 — gateway page points to Data Sources for external-data queries.
  const navigate = useNavigate()

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
      <p className="text-on-surface-variant font-body-sm max-w-prose">
        {t('settings.connections.subtitle')}
      </p>

      {/* X3 互链: gateway = model/platform access channel, data sources =
          external data connections. One line each way so users stop bouncing
          between the two pages looking for "where do I connect X". */}
      <p className="text-on-surface-variant/80 font-body-sm max-w-prose flex flex-wrap items-center gap-xs">
        <span className="material-symbols-outlined icon-sm" aria-hidden="true">
          swap_horiz
        </span>
        {t('settings.connections.crossLink.text')}{' '}
        <button
          type="button"
          onClick={() => navigate('/extensions/datasources')}
          data-testid="gateway-to-datasources-link"
          className="text-primary hover:underline cursor-pointer inline-flex items-center gap-0.5"
        >
          {t('settings.connections.crossLink.link')}
          <span className="material-symbols-outlined text-[14px]" aria-hidden="true">
            arrow_forward
          </span>
        </button>
      </p>

      <EngineConnectionCard
        config={config}
        engineDraft={engineDraft}
        onEngineDraftChange={setEngineDraft}
        onConfigChange={setConfig}
      />

      <GatewayProcessCard procState={procState} onProcStateChange={setProcState} />

      <MobileDispatchCard config={config} procState={procState} onConfigChange={setConfig} />

      <PlatformsCard
        config={config}
        hasSecret={hasSecret}
        drafts={drafts}
        saving={saving}
        procState={procState}
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
        onProcStateChange={setProcState}
      />
    </div>
  )
}
