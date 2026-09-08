import { useState } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'

import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import { toastError } from '@/lib/errorToast'
import * as api from '@/lib/tauri-api'
import type { GatewayConfig, GatewayProcessState } from '@/types'
import {
  PLATFORMS,
  PLATFORM_LABEL,
  SECRET_MODEL,
  platformStatus,
  readTrigger,
  withTrigger,
  type Platform,
  type PlatformStatus,
} from './types'
import { cn } from '@/lib/utils'

interface PlatformsCardProps {
  config: GatewayConfig
  hasSecret: Record<string, boolean>
  drafts: Record<string, string>
  saving: Platform | null
  procState: GatewayProcessState | null
  onDraftChange: (key: string, value: string) => void
  onSavedDrafts: (keys: string[]) => void
  onSavingChange: (next: Platform | null) => void
  onConfigChange: (next: GatewayConfig) => void
  onHasSecretChange: (next: Record<string, boolean>) => void
  onProcStateChange: (next: GatewayProcessState) => void
}

const STATUS_DOT: Record<PlatformStatus, string> = {
  notConfigured: 'bg-outline-variant/50',
  configured: 'bg-primary',
  running: 'bg-tertiary',
  error: 'bg-destructive',
}

export function PlatformsCard({
  config,
  hasSecret,
  drafts,
  saving,
  procState,
  onDraftChange,
  onSavedDrafts,
  onSavingChange,
  onConfigChange,
  onHasSecretChange,
  onProcStateChange,
}: PlatformsCardProps) {
  const intl = useIntl()
  const t = (id: string): string => intl.formatMessage({ id })
  const [restarting, setRestarting] = useState(false)
  // Config/secret writes only reach the gateway process on restart; track
  // whether the shown state may be stale so the user gets a one-click reload.
  const [needsRestart, setNeedsRestart] = useState(false)

  const adapterFor = (p: Platform) => config.adapters.find((a) => a.platform === p)
  const isEnabled = (p: Platform): boolean => adapterFor(p)?.enabled ?? false

  // A platform is "configured" once every required slot has a stored value.
  const isPlatformConnected = (p: Platform): boolean =>
    SECRET_MODEL[p].filter((s) => s.required).every((s) => hasSecret[s.key] ?? false)

  const platformHasDraft = (p: Platform): boolean =>
    SECRET_MODEL[p].some((s) => (drafts[s.key] ?? '').trim())

  const gatewayRunning =
    typeof procState?.status === 'object' &&
    procState?.status !== null &&
    'running' in procState.status

  const statusOf = (p: Platform): PlatformStatus =>
    platformStatus({
      hasAllRequiredSecrets: isPlatformConnected(p),
      enabled: isEnabled(p),
      supervisor: procState?.status ?? 'stopped',
    })

  function markStale(): void {
    if (gatewayRunning) setNeedsRestart(true)
  }

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
      markStale()
    } catch (e) {
      toastError('keyring: save failed', e)
    } finally {
      onSavingChange(null)
    }
  }

  async function toggleEnable(p: Platform, enabled: boolean): Promise<void> {
    const existing = adapterFor(p)
    const others = config.adapters.filter((a) => a.platform !== p)
    // Keep the entry (with its options/secrets mapping) on disable so trigger
    // toggles survive an off→on cycle; the gateway skips disabled adapters.
    const next: GatewayConfig = {
      ...config,
      adapters: enabled
        ? [
            ...others,
            {
              platform: p,
              enabled: true,
              options: existing?.options,
              // Every slot's adapter-local name → its OS-keyring key.
              secrets:
                existing?.secrets ??
                Object.fromEntries(SECRET_MODEL[p].map((s) => [s.name, s.key])),
            },
          ]
        : [...others, { platform: p, enabled: false, options: existing?.options, secrets: existing?.secrets }],
    }
    try {
      const written = await api.gatewayWriteConfig(next)
      onConfigChange(written)
      markStale()
    } catch (e) {
      toastError('gateway config: write failed', e)
    }
  }

  /** Persist a trigger-policy change for one platform (P1-4). */
  async function toggleTrigger(
    p: Platform,
    patch: { groupMode?: 'mentionOrPrefix' | 'any'; dmDirect?: boolean },
  ): Promise<void> {
    const existing = adapterFor(p)
    if (!existing) return
    const current = readTrigger(existing.options)
    const nextTrigger = { ...current, ...patch }
    const next: GatewayConfig = {
      ...config,
      adapters: config.adapters.map((a) =>
        a.platform === p ? { ...a, options: withTrigger(a.options, nextTrigger) } : a,
      ),
    }
    try {
      const written = await api.gatewayWriteConfig(next)
      onConfigChange(written)
      markStale()
    } catch (e) {
      toastError('gateway config: write failed', e)
    }
  }

  /** Whole-gateway restart — v1 reload semantics (no per-adapter hot reload). */
  async function restartGateway(): Promise<void> {
    setRestarting(true)
    try {
      await api.gatewaySupervisorStop()
      const s = await api.gatewaySupervisorStart()
      onProcStateChange(s)
      setNeedsRestart(false)
      toast.success(t('settings.connections.restart.restarted'))
    } catch (e) {
      toastError('gateway supervisor: restart failed', e)
    } finally {
      setRestarting(false)
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
          const trigger = readTrigger(adapterFor(p)?.options)
          const status = statusOf(p)
          return (
            <div
              key={p}
              data-testid={`connection-${p}`}
              className="flex flex-col gap-sm pt-md first:pt-0"
            >
              <div className="flex items-center justify-between gap-md">
                <div className="flex items-center gap-sm">
                  <span className="font-label-md text-on-surface">{PLATFORM_LABEL[p]}</span>
                  <span
                    data-testid={`connection-status-${p}`}
                    className={cn('h-2 w-2 shrink-0 rounded-full', STATUS_DOT[status])}
                    aria-hidden="true"
                  />
                  <span className="font-label-sm text-on-surface-variant">
                    {t(`settings.connections.status.${status}`)}
                  </span>
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
                      {/* type=password + draft-only value: the webview never
                          echoes or re-reads the stored secret — only its
                          keyring presence is probed and shown as "Set". */}
                      <Input
                        type="password"
                        aria-label={label}
                        placeholder={t(s.labelKey)}
                        value={drafts[s.key] ?? ''}
                        onChange={(e) => onDraftChange(s.key, e.target.value)}
                        spellCheck={false}
                      />
                      <code className="font-label-sm text-on-surface-variant whitespace-nowrap">
                        {s.key}
                      </code>
                      {present && (
                        <span
                          data-testid={`secret-set-${s.key}`}
                          className="font-label-sm text-on-surface-variant whitespace-nowrap"
                        >
                          ✓ {t('settings.connections.secret.set')}
                        </span>
                      )}
                    </div>
                  </div>
                )
              })}
              <div className="flex flex-wrap items-center gap-md">
                <Button
                  variant="secondary"
                  onClick={() => savePlatform(p)}
                  disabled={saving === p || saving !== null || !platformHasDraft(p)}
                >
                  {t('settings.connections.save')}
                </Button>
                <Button
                  variant="secondary"
                  disabled
                  title={t('settings.connections.test.unsupported')}
                >
                  {t('settings.connections.test.button')}
                </Button>
              </div>
              {/* P1-4 trigger policy (IM inbound): group chats need @mention or
                  the /shannon prefix; DMs answer directly. The switches write
                  AdapterConfig.options.trigger and only apply once the adapter
                  entry exists (enable the platform first). */}
              <div className="flex flex-col gap-xs">
                <div className="flex items-center justify-between gap-md">
                  <label
                    htmlFor={`gw-trigger-mention-${p}`}
                    className="font-label-sm text-on-surface-variant"
                    title={t('settings.connections.trigger.groupMentionHint')}
                  >
                    {t('settings.connections.trigger.groupMention')}
                  </label>
                  <Switch
                    id={`gw-trigger-mention-${p}`}
                    disabled={!adapterFor(p)}
                    checked={trigger.groupMode !== 'any'}
                    onCheckedChange={(checked) =>
                      toggleTrigger(p, { groupMode: checked ? 'mentionOrPrefix' : 'any' })
                    }
                  />
                </div>
                <div className="flex items-center justify-between gap-md">
                  <label
                    htmlFor={`gw-trigger-dm-${p}`}
                    className="font-label-sm text-on-surface-variant"
                    title={t('settings.connections.trigger.dmDirectHint')}
                  >
                    {t('settings.connections.trigger.dmDirect')}
                  </label>
                  <Switch
                    id={`gw-trigger-dm-${p}`}
                    disabled={!adapterFor(p)}
                    checked={trigger.dmDirect !== false}
                    onCheckedChange={(checked) => toggleTrigger(p, { dmDirect: checked })}
                  />
                </div>
              </div>
            </div>
          )
        })}
        {needsRestart && gatewayRunning && (
          <div className="flex items-center justify-between gap-md border-t border-surface-border pt-md">
            <span className="font-body-sm text-on-surface-variant">
              {t('settings.connections.restart.required')}
            </span>
            <Button variant="secondary" onClick={restartGateway} disabled={restarting}>
              {t('settings.connections.restart.now')}
            </Button>
          </div>
        )}
      </CardContent>
    </Card>
  )
}
