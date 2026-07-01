import { useEffect, useState } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import { useTauriEvent } from '@/hooks/useTauriEvent'
import { toastError } from '@/lib/errorToast'
import * as api from '@/lib/tauri-api'
import type { GatewayConfig, GatewayProcessState, GatewaySupervisorStatus } from '@/types'

// The gateway platforms (mirrors the `Platform` enum in
// shannon-gateway/src/adapters/types.ts). Order = display order.
const PLATFORMS = [
  'slack',
  'telegram',
  'discord',
  'matrix',
  'whatsapp',
  'wecom',
  'feishu',
  'dingtalk',
] as const
type Platform = (typeof PLATFORMS)[number]

const PLATFORM_LABEL: Record<Platform, string> = {
  slack: 'Slack',
  telegram: 'Telegram',
  discord: 'Discord',
  matrix: 'Matrix',
  whatsapp: 'WhatsApp',
  wecom: 'WeCom (企业微信)',
  feishu: 'Feishu (飞书)',
  dingtalk: 'DingTalk (钉钉)',
}

// One credential slot the gateway reads from the OS keyring at adapter
// start(). The `name` is the adapter-local key (it becomes the entry in
// GatewayAdapter.secrets), and `key` is the exact OS-keyring key the
// gateway's ctx.getSecret(...) call reads. These keys are verified against
// each adapter's start() in shannon-gateway/src/adapters/*.
//
// `required` mirrors whether the adapter throws when the slot is missing:
// a platform is "connected" once every required slot has a stored value.
interface SecretSlot {
  name: string
  key: string
  labelKey: string
  required: boolean
}

const slot = (name: string, key: string, labelKey: string, required: boolean): SecretSlot => ({
  name,
  key,
  labelKey,
  required,
})

const S = (n: string) => `settings.connections.secret.${n}.label`

const SECRET_MODEL: Record<Platform, SecretSlot[]> = {
  slack: [
    slot('botToken', 'slack/bot-token', S('botToken'), true),
    slot('signingSecret', 'slack/signing-secret', S('signingSecret'), true),
  ],
  telegram: [slot('botToken', 'telegram/bot-token', S('botToken'), true)],
  discord: [slot('botToken', 'discord/bot-token', S('botToken'), true)],
  matrix: [slot('accessToken', 'matrix/access-token', S('accessToken'), true)],
  whatsapp: [
    slot('accessToken', 'whatsapp/access-token', S('accessToken'), true),
    slot('appSecret', 'whatsapp/app-secret', S('appSecret'), false),
  ],
  wecom: [
    slot('corpSecret', 'wecom/corp-secret', S('corpSecret'), true),
    slot('encodingAesKey', 'wecom/encoding-aes-key', S('encodingAesKey'), true),
  ],
  feishu: [
    slot('appSecret', 'feishu/app-secret', S('appSecret'), true),
    slot('encryptKey', 'feishu/encrypt-key', S('encryptKey'), false),
  ],
  dingtalk: [slot('robotSecret', 'dingtalk/robot-secret', S('robotSecret'), true)],
}

// Flatten once for the keyring-presence probe.
const ALL_SLOTS = PLATFORMS.flatMap((p) => SECRET_MODEL[p].map((s) => ({ p, s })))

export default function ConnectionsSettings() {
  const intl = useIntl()
  const t = (id: string): string => intl.formatMessage({ id })

  const [config, setConfig] = useState<GatewayConfig | null>(null)
  const [hasSecret, setHasSecret] = useState<Record<string, boolean>>({})
  const [drafts, setDrafts] = useState<Record<string, string>>({})
  const [saving, setSaving] = useState<Platform | null>(null)
  const [engineDraft, setEngineDraft] = useState({ wsUrl: '', httpBaseUrl: '' })
  const [savingEngine, setSavingEngine] = useState(false)

  // E-1 方案 C — supervised gateway process state.
  const [procState, setProcState] = useState<GatewayProcessState | null>(null)
  const [procBusy, setProcBusy] = useState<'start' | 'stop' | 'managed' | null>(null)

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

  const isEnabled = (p: Platform): boolean =>
    config?.adapters.some((a) => a.platform === p && a.enabled) ?? false

  // A platform is "connected" once every required slot has a stored value.
  const isPlatformConnected = (p: Platform): boolean =>
    SECRET_MODEL[p].filter((s) => s.required).every((s) => hasSecret[s.key] ?? false)

  const platformHasDraft = (p: Platform): boolean =>
    SECRET_MODEL[p].some((s) => (drafts[s.key] ?? '').trim())

  async function savePlatform(p: Platform): Promise<void> {
    const slots = SECRET_MODEL[p]
    const entries = slots.filter((s) => (drafts[s.key] ?? '').trim())
    if (!entries.length) return
    setSaving(p)
    try {
      await Promise.all(
        entries.map((s) => api.gatewaySetSecret(s.key, (drafts[s.key] ?? '').trim())),
      )
      setHasSecret((m) => ({
        ...m,
        ...Object.fromEntries(entries.map((s) => [s.key, true])),
      }))
      setDrafts((d) => {
        const next = { ...d }
        entries.forEach((s) => delete next[s.key])
        return next
      })
      toast.success(t('settings.connections.saved'))
    } catch (e) {
      toastError('keyring: save failed', e)
    } finally {
      setSaving(null)
    }
  }

  async function toggleEnable(p: Platform, enabled: boolean): Promise<void> {
    if (!config) return
    const others = config.adapters.filter((a) => a.platform !== p)
    const next: GatewayConfig = enabled
      ? {
          ...config,
          adapters: [
            ...others,
            {
              platform: p,
              enabled: true,
              // Every slot's adapter-local name → its OS-keyring key.
              secrets: Object.fromEntries(SECRET_MODEL[p].map((s) => [s.name, s.key])),
            },
          ],
        }
      : { ...config, adapters: others }
    try {
      const written = await api.gatewayWriteConfig(next)
      setConfig(written)
    } catch (e) {
      toastError('gateway config: write failed', e)
    }
  }

  async function saveEngine(): Promise<void> {
    if (!config) return
    const next: GatewayConfig = {
      ...config,
      engine: { ...config.engine, ...engineDraft },
    }
    setSavingEngine(true)
    try {
      const written = await api.gatewayWriteConfig(next)
      setConfig(written)
      toast.success(t('settings.connections.engineSaved'))
    } catch (e) {
      toastError('gateway config: write failed', e)
    } finally {
      setSavingEngine(false)
    }
  }

  // E-1 方案 C handlers.
  async function startGateway(): Promise<void> {
    setProcBusy('start')
    try {
      const s = await api.gatewaySupervisorStart()
      setProcState(s)
      toast.success(t('settings.connections.process.started'))
    } catch (e) {
      toastError('gateway supervisor: start failed', e)
    } finally {
      setProcBusy(null)
    }
  }

  async function stopGateway(): Promise<void> {
    setProcBusy('stop')
    try {
      const s = await api.gatewaySupervisorStop()
      setProcState(s)
      toast.success(t('settings.connections.process.stopped'))
    } catch (e) {
      toastError('gateway supervisor: stop failed', e)
    } finally {
      setProcBusy(null)
    }
  }

  async function toggleManaged(managed: boolean): Promise<void> {
    setProcBusy('managed')
    try {
      const s = await api.gatewaySetManaged(managed)
      setProcState(s)
      toast.success(t('settings.connections.process.managedSaved'))
    } catch (e) {
      toastError('gateway supervisor: set managed failed', e)
    } finally {
      setProcBusy(null)
    }
  }

  const procManaged = procState?.managed ?? true
  const procStatus: GatewaySupervisorStatus = procState?.status ?? 'stopped'

  const procBadge = (() => {
    const s = procStatus
    if (s === 'stopped')
      return { variant: 'neutral' as const, label: t('settings.connections.process.statusStopped') }
    if (s === 'notInstalled')
      return { variant: 'warning' as const, label: t('settings.connections.process.statusNotInstalled') }
    if (typeof s === 'object' && 'running' in s)
      return {
        variant: 'success' as const,
        label: `${t('settings.connections.process.statusRunning')} (PID ${s.running.pid})`,
      }
    return {
      variant: 'error' as const,
      label: `${t('settings.connections.process.statusExited')}: ${s.exited.reason}`,
    }
  })()

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

      {/* Engine connection */}
      <Card>
        <CardHeader>
          <CardTitle>{t('settings.connections.engineTitle')}</CardTitle>
          <CardDescription>{t('settings.connections.gatewayHint')}</CardDescription>
        </CardHeader>
        <CardContent className="space-y-md">
          <div className="space-y-1">
            <label htmlFor="gw-wsurl" className="font-label-sm text-on-surface-variant block">
              {t('settings.connections.wsUrl')}
            </label>
            <Input
              id="gw-wsurl"
              value={engineDraft.wsUrl}
              onChange={(e) => setEngineDraft((d) => ({ ...d, wsUrl: e.target.value }))}
              spellCheck={false}
            />
          </div>
          <div className="space-y-1">
            <label htmlFor="gw-http" className="font-label-sm text-on-surface-variant block">
              {t('settings.connections.httpBaseUrl')}
            </label>
            <Input
              id="gw-http"
              value={engineDraft.httpBaseUrl}
              onChange={(e) => setEngineDraft((d) => ({ ...d, httpBaseUrl: e.target.value }))}
              spellCheck={false}
            />
          </div>
          <Button onClick={saveEngine} disabled={savingEngine}>
            {t('settings.connections.saveEngine')}
          </Button>
        </CardContent>
      </Card>

      {/* E-1 方案 C — Gateway process lifecycle */}
      <Card>
        <CardHeader>
          <CardTitle>{t('settings.connections.process.title')}</CardTitle>
          <CardDescription>{t('settings.connections.process.description')}</CardDescription>
        </CardHeader>
        <CardContent className="space-y-md">
          <div className="flex items-center justify-between gap-md">
            <div className="space-y-1">
              <span className="font-label-md text-on-surface">
                {t('settings.connections.process.managedLabel')}
              </span>
              <p className="font-body-sm text-on-surface-variant max-w-prose">
                {t('settings.connections.process.managedHint')}
              </p>
            </div>
            <Switch
              data-testid="gateway-managed-switch"
              aria-label={t('settings.connections.process.managedLabel')}
              checked={procManaged}
              disabled={procBusy === 'managed'}
              onCheckedChange={toggleManaged}
            />
          </div>

          {procManaged && (
            <div className="flex flex-wrap items-center justify-between gap-md border-t border-surface-border pt-md">
              <div className="flex items-center gap-sm">
                <span className="font-label-sm text-on-surface-variant">
                  {t('settings.connections.process.status')}
                </span>
                <Badge variant={procBadge.variant} data-testid="gateway-status-badge">
                  {procBadge.label}
                </Badge>
              </div>
              <div className="flex items-center gap-sm">
                <Button
                  variant="secondary"
                  onClick={startGateway}
                  disabled={procBusy !== null}
                >
                  {t('settings.connections.process.start')}
                </Button>
                <Button
                  variant="secondary"
                  onClick={stopGateway}
                  disabled={procBusy !== null}
                >
                  {t('settings.connections.process.stop')}
                </Button>
              </div>
            </div>
          )}

          {procManaged && procStatus === 'notInstalled' && (
            <p className="font-body-sm text-on-surface-variant max-w-prose">
              {t('settings.connections.process.notInstalledHint')}
            </p>
          )}
        </CardContent>
      </Card>

      {/* Platforms */}
      <Card>
        <CardHeader>
          <CardTitle>{t('settings.connections.platformsTitle')}</CardTitle>
          <CardDescription>{t('settings.connections.keyringNote')}</CardDescription>
        </CardHeader>
        <CardContent className="space-y-md divide-y divide-surface-border">
          {PLATFORMS.map((p) => {
            const connected = isPlatformConnected(p)
            return (
              <div
                key={p}
                data-testid={`connection-${p}`}
                className="flex flex-col gap-sm pt-md first:pt-0"
              >
                <div className="flex items-center justify-between gap-md">
                  <div className="flex items-center gap-sm">
                    <span className="font-label-md text-on-surface">{PLATFORM_LABEL[p]}</span>
                    <Badge variant={connected ? 'success' : 'neutral'}>
                      {connected
                        ? t('settings.connections.connected')
                        : t('settings.connections.notConnected')}
                    </Badge>
                  </div>
                  <div className="flex items-center gap-sm">
                    <label htmlFor={`gw-enable-${p}`} className="font-label-sm text-on-surface-variant">
                      {t('settings.connections.enable')}
                    </label>
                    <Switch
                      id={`gw-enable-${p}`}
                      checked={isEnabled(p)}
                      onCheckedChange={(checked) => toggleEnable(p, checked)}
                    />
                  </div>
                </div>
                {SECRET_MODEL[p].map((s) => {
                  const present = hasSecret[s.key] ?? false
                  const label = `${t(s.labelKey)}${s.required ? '' : t('settings.connections.secret.optionalSuffix')} — ${PLATFORM_LABEL[p]}`
                  return (
                    <div key={s.key} className="flex flex-col gap-xs">
                      <div className="flex items-center gap-sm">
                        <Input
                          type="password"
                          aria-label={label}
                          placeholder={t(s.labelKey)}
                          value={drafts[s.key] ?? ''}
                          onChange={(e) => setDrafts((d) => ({ ...d, [s.key]: e.target.value }))}
                          spellCheck={false}
                        />
                        <span
                          aria-hidden="true"
                          className={`h-2 w-2 shrink-0 rounded-full ${present ? 'bg-primary' : 'bg-outline-variant/50'}`}
                          title={present ? t('settings.connections.connected') : t('settings.connections.notConnected')}
                        />
                        <code className="font-label-sm text-on-surface-variant whitespace-nowrap">
                          {s.key}
                        </code>
                      </div>
                    </div>
                  )
                })}
                <Button
                  variant="secondary"
                  onClick={() => savePlatform(p)}
                  disabled={saving === p || saving !== null || !platformHasDraft(p)}
                >
                  {t('settings.connections.save')}
                </Button>
              </div>
            )
          })}
        </CardContent>
      </Card>
    </div>
  )
}
