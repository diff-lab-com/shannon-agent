import { useEffect, useState } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import { toastError } from '@/lib/errorToast'
import * as api from '@/lib/tauri-api'
import type { GatewayConfig } from '@/types'

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

// Primary bot-token credential key for a platform. The gateway reads this
// exact key from the OS keyring at adapter start() (see
// shannon-gateway/src/secrets/cliKeyring.ts).
const secretKey = (p: Platform): string => `${p}/bot-token`

const SECRET_NAME = 'botToken'

export default function ConnectionsSettings() {
  const intl = useIntl()
  const t = (id: string): string => intl.formatMessage({ id })

  const [config, setConfig] = useState<GatewayConfig | null>(null)
  const [hasSecret, setHasSecret] = useState<Record<string, boolean>>({})
  const [drafts, setDrafts] = useState<Record<string, string>>({})
  const [saving, setSaving] = useState<Platform | null>(null)
  const [engineDraft, setEngineDraft] = useState({ wsUrl: '', httpBaseUrl: '' })
  const [savingEngine, setSavingEngine] = useState(false)

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

  // Probe each platform's keyring presence so the UI can show a badge without
  // ever pulling the secret value into the webview.
  useEffect(() => {
    let cancelled = false
    Promise.all(
      PLATFORMS.map((p) =>
        api.gatewayHasSecret(secretKey(p)).then((present) => [p, present] as [string, boolean]),
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

  async function saveSecret(p: Platform): Promise<void> {
    const value = drafts[p] ?? ''
    if (!value.trim()) return
    setSaving(p)
    try {
      await api.gatewaySetSecret(secretKey(p), value.trim())
      setHasSecret((m) => ({ ...m, [p]: true }))
      setDrafts((d) => ({ ...d, [p]: '' }))
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
            { platform: p, enabled: true, secrets: { [SECRET_NAME]: secretKey(p) } },
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

      {/* Platforms */}
      <Card>
        <CardHeader>
          <CardTitle>{t('settings.connections.platformsTitle')}</CardTitle>
          <CardDescription>{t('settings.connections.keyringNote')}</CardDescription>
        </CardHeader>
        <CardContent className="space-y-md divide-y divide-surface-border">
          {PLATFORMS.map((p) => {
            const present = hasSecret[p] ?? false
            return (
              <div key={p} data-testid={`connection-${p}`} className="flex flex-col gap-sm pt-md first:pt-0">
                <div className="flex items-center justify-between gap-md">
                  <div className="flex items-center gap-sm">
                    <span className="font-label-md text-on-surface">{PLATFORM_LABEL[p]}</span>
                    <Badge variant={present ? 'success' : 'neutral'}>
                      {present
                        ? t('settings.connections.connected')
                        : t('settings.connections.notConnected')}
                    </Badge>
                    <code className="font-label-sm text-on-surface-variant">
                      {secretKey(p)}
                    </code>
                  </div>
                  <div className="flex items-center gap-sm">
                    <label
                      htmlFor={`gw-enable-${p}`}
                      className="font-label-sm text-on-surface-variant"
                    >
                      {t('settings.connections.enable')}
                    </label>
                    <Switch
                      id={`gw-enable-${p}`}
                      checked={isEnabled(p)}
                      onCheckedChange={(checked) => toggleEnable(p, checked)}
                    />
                  </div>
                </div>
                <div className="flex items-center gap-sm">
                  <Input
                    type="password"
                    aria-label={`${t('settings.connections.tokenLabel')} — ${PLATFORM_LABEL[p]}`}
                    placeholder={t('settings.connections.tokenPlaceholder')}
                    value={drafts[p] ?? ''}
                    onChange={(e) => setDrafts((d) => ({ ...d, [p]: e.target.value }))}
                    spellCheck={false}
                  />
                  <Button
                    variant="secondary"
                    onClick={() => saveSecret(p)}
                    disabled={saving === p || !(drafts[p] ?? '').trim()}
                  >
                    {t('settings.connections.save')}
                  </Button>
                </div>
              </div>
            )
          })}
        </CardContent>
      </Card>
    </div>
  )
}
