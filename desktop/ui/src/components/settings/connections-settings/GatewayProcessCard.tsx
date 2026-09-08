import { useState } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Switch } from '@/components/ui/switch'
import { toastError } from '@/lib/errorToast'
import * as api from '@/lib/tauri-api'
import type { GatewayProcessState, GatewaySupervisorStatus } from '@/types'

interface GatewayProcessCardProps {
  procState: GatewayProcessState | null
  onProcStateChange: (next: GatewayProcessState) => void
}

export function GatewayProcessCard({ procState, onProcStateChange }: GatewayProcessCardProps) {
  const intl = useIntl()
  const t = (id: string): string => intl.formatMessage({ id })
  const [procBusy, setProcBusy] = useState<'start' | 'stop' | 'managed' | null>(null)

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

  async function startGateway(): Promise<void> {
    setProcBusy('start')
    try {
      const s = await api.gatewaySupervisorStart()
      onProcStateChange(s)
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
      onProcStateChange(s)
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
      onProcStateChange(s)
      toast.success(t('settings.connections.process.managedSaved'))
    } catch (e) {
      toastError('gateway supervisor: set managed failed', e)
    } finally {
      setProcBusy(null)
    }
  }

  return (
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
  )
}
