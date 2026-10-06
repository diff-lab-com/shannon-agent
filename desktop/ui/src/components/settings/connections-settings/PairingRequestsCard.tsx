import { useCallback, useEffect, useState } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'

import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { toastError } from '@/lib/errorToast'
import * as api from '@/lib/tauri-api'
import type { GatewayConfig, GatewayPairingRequest, GatewayProcessState } from '@/types'

interface PairingRequestsCardProps {
  /** Live gateway config — `mobile.enabled` gates the RPC surface this uses. */
  config: GatewayConfig
  /** Supervised gateway process state — a stopped gateway has no fresh codes. */
  procState: GatewayProcessState | null
}

/** Whether the supervised gateway process is currently running. */
function isGatewayRunning(procState: GatewayProcessState | null): boolean {
  const status = procState?.status
  return typeof status === 'object' && status !== null && 'running' in status
}

/**
 * 「IM 配对请求」card (T9, review F42 desktop entry): lists the pending IM
 * pairing challenges (B6) the gateway is holding and approves them in place —
 * the desktop alternative to replying `approve <code>` from an already-paired
 * sender. Both channels hit the same gateway store; this one authenticates
 * with a freshly minted pair token per call.
 */
export function PairingRequestsCard({ config, procState }: PairingRequestsCardProps) {
  const intl = useIntl()
  const t = (id: string): string => intl.formatMessage({ id })
  const tVal = (id: string, values: Record<string, string | number>): string =>
    intl.formatMessage({ id }, values)

  const [requests, setRequests] = useState<GatewayPairingRequest[]>([])
  const [approving, setApproving] = useState<string | null>(null)
  const [nowMs, setNowMs] = useState(() => Date.now())

  const enabled = config.mobile?.enabled === true
  const gatewayRunning = isGatewayRunning(procState)

  const refresh = useCallback(async (): Promise<void> => {
    try {
      setRequests(await api.gatewayPairingPending())
    } catch {
      /* best-effort: the gateway may be down or mid-restart; keep the list */
    }
  }, [])

  // The page's data pattern: fetch on mount (and when the gateway process
  // (re)starts) + a manual refresh button.
  useEffect(() => {
    if (!enabled) return
    void refresh()
  }, [enabled, gatewayRunning, refresh])

  // Tick the expiry countdowns once per second while requests are on screen.
  useEffect(() => {
    if (requests.length === 0) return
    const id = window.setInterval(() => setNowMs(Date.now()), 1000)
    return () => window.clearInterval(id)
  }, [requests.length])

  async function approve(request: GatewayPairingRequest): Promise<void> {
    setApproving(request.code)
    try {
      await api.gatewayPairingApprove(request.code)
      toast.success(
        tVal('settings.connections.imPairing.approved', {
          sender: `${request.platform}:${request.senderId}`,
        }),
      )
      await refresh()
    } catch (e) {
      // The gateway's reason (unknown/expired code) lands in the error toast.
      toastError(t('settings.connections.imPairing.approveFailed'), e)
    } finally {
      setApproving(null)
    }
  }

  if (!enabled) return null

  return (
    <Card data-testid="pairing-requests-card">
      <CardHeader>
        <CardTitle>{t('settings.connections.imPairing.title')}</CardTitle>
        <CardDescription>{t('settings.connections.imPairing.description')}</CardDescription>
      </CardHeader>
      <CardContent className="space-y-md">
        <div className="flex items-center gap-sm">
          <Button
            variant="secondary"
            onClick={() => void refresh()}
            data-testid="pairing-requests-refresh"
          >
            {t('settings.connections.imPairing.refresh')}
          </Button>
          {!gatewayRunning && (
            <span
              className="font-label-xs text-on-surface-variant"
              data-testid="pairing-requests-stopped"
            >
              {t('settings.connections.imPairing.gatewayStopped')}
            </span>
          )}
        </div>

        {requests.length === 0 ? (
          <p className="font-body-sm text-on-surface-variant" data-testid="pairing-requests-empty">
            {t('settings.connections.imPairing.empty')}
          </p>
        ) : (
          <ul className="space-y-sm" data-testid="pairing-requests-list">
            {requests.map((request) => {
              const seconds = Math.max(0, Math.ceil((request.expiresAt - nowMs) / 1000))
              return (
                <li
                  key={`${request.platform}:${request.senderId}:${request.code}`}
                  className="flex items-center justify-between gap-md"
                  data-testid={`pairing-request-${request.code}`}
                >
                  <div className="flex flex-col">
                    <span className="font-label-sm text-on-surface">
                      {request.platform}:{request.senderId}
                    </span>
                    <code className="font-label-xs text-on-surface-variant">
                      {request.code} ·{' '}
                      {tVal('settings.connections.imPairing.expiresIn', { seconds })}
                    </code>
                  </div>
                  <Button
                    onClick={() => void approve(request)}
                    disabled={approving !== null}
                    data-testid={`pairing-approve-${request.code}`}
                  >
                    {approving === request.code
                      ? t('settings.connections.imPairing.approving')
                      : t('settings.connections.imPairing.approve')}
                  </Button>
                </li>
              )
            })}
          </ul>
        )}
      </CardContent>
    </Card>
  )
}
