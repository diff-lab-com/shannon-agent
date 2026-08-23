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
import { PLATFORMS, PLATFORM_LABEL, SECRET_MODEL, type Platform } from './types'

interface PlatformsCardProps {
  config: GatewayConfig
  hasSecret: Record<string, boolean>
  drafts: Record<string, string>
  saving: Platform | null
  onDraftChange: (key: string, value: string) => void
  onSavedDrafts: (keys: string[]) => void
  onSavingChange: (next: Platform | null) => void
  onConfigChange: (next: GatewayConfig) => void
  onHasSecretChange: (next: Record<string, boolean>) => void
}

export function PlatformsCard({
  config,
  hasSecret,
  drafts,
  saving,
  onDraftChange,
  onSavedDrafts,
  onSavingChange,
  onConfigChange,
  onHasSecretChange,
}: PlatformsCardProps) {
  const intl = useIntl()
  const t = (id: string): string => intl.formatMessage({ id })

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
    onSavingChange(p)
    try {
      await Promise.all(
        entries.map((s) => api.gatewaySetSecret(s.key, (drafts[s.key] ?? '').trim())),
      )
      onHasSecretChange({
        ...hasSecret,
        ...Object.fromEntries(entries.map((s) => [s.key, true])),
      })
      onSavedDrafts(entries.map((s) => s.key))
      toast.success(t('settings.connections.saved'))
    } catch (e) {
      toastError('keyring: save failed', e)
    } finally {
      onSavingChange(null)
    }
  }

  async function toggleEnable(p: Platform, enabled: boolean): Promise<void> {
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
      onConfigChange(written)
    } catch (e) {
      toastError('gateway config: write failed', e)
    }
  }

  return (
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
                        onChange={(e) => onDraftChange(s.key, e.target.value)}
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
  )
}
