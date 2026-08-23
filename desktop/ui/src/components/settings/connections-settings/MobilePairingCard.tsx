import { useEffect, useState } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'

import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import { toastError } from '@/lib/errorToast'
import * as api from '@/lib/tauri-api'
import type { MobileDeviceEntry, MobilePairToken } from '@/types'

export function MobilePairingCard() {
  const intl = useIntl()
  const t = (id: string): string => intl.formatMessage({ id })
  const tVal = (id: string, values: Record<string, string | number>): string =>
    intl.formatMessage({ id }, values)

  const [pairToken, setPairToken] = useState<MobilePairToken | null>(null)
  const [pairBusy, setPairBusy] = useState(false)
  const [pairError, setPairError] = useState<string | null>(null)
  const [pairedDevices, setPairedDevices] = useState<MobileDeviceEntry[]>([])
  const [revokeTarget, setRevokeTarget] = useState<MobileDeviceEntry | null>(null)
  const [revokeBusy, setRevokeBusy] = useState(false)
  const [nowMs, setNowMs] = useState(() => Date.now())

  // P1.3 — load the paired-device registry once on mount.
  useEffect(() => {
    api.mobileListPairedDevices().then(setPairedDevices).catch(() => {})
  }, [])

  // Tick the countdown once per second only while a QR is on screen.
  useEffect(() => {
    if (!pairToken) return
    const id = window.setInterval(() => setNowMs(Date.now()), 1000)
    return () => window.clearInterval(id)
  }, [pairToken])

  // QR countdown — the gateway consumes the one-time token within its TTL.
  const tokenExpired = pairToken !== null && nowMs >= pairToken.expiresAt
  const remainingSec = pairToken
    ? Math.max(0, Math.ceil((pairToken.expiresAt - nowMs) / 1000))
    : 0

  async function generatePairToken(): Promise<void> {
    setPairBusy(true)
    setPairError(null)
    try {
      const tok = await api.mobileGeneratePairToken()
      setPairToken(tok)
      setNowMs(Date.now())
      // Refresh the registry in case a prior pairing just completed.
      api.mobileListPairedDevices().then(setPairedDevices).catch(() => {})
    } catch (e) {
      setPairError(typeof e === 'string' ? e : (e as Error)?.message ?? String(e))
      setPairToken(null)
    } finally {
      setPairBusy(false)
    }
  }

  async function refreshDevices(): Promise<void> {
    try {
      setPairedDevices(await api.mobileListPairedDevices())
    } catch {
      /* best-effort; the list just stays stale */
    }
  }

  async function confirmRevoke(): Promise<void> {
    if (!revokeTarget) return
    const target = revokeTarget
    setRevokeBusy(true)
    try {
      await api.mobileRevokeDevice(target.deviceId)
      setPairedDevices((ds) => ds.filter((d) => d.deviceId !== target.deviceId))
      toast.success(t('settings.connections.mobile.revoked'))
    } catch (e) {
      toastError('mobile: revoke failed', e)
    } finally {
      setRevokeBusy(false)
      setRevokeTarget(null)
    }
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>{t('settings.connections.mobile.title')}</CardTitle>
        <CardDescription>{t('settings.connections.mobile.description')}</CardDescription>
      </CardHeader>
      <CardContent className="space-y-md">
        <div className="flex flex-wrap items-start gap-md">
          <div className="flex items-center gap-sm">
            <Button
              onClick={generatePairToken}
              disabled={pairBusy}
              data-testid="mobile-pair-generate"
            >
              {t('settings.connections.mobile.generate')}
            </Button>
            <Button
              variant="secondary"
              onClick={refreshDevices}
              data-testid="mobile-pair-refresh"
            >
              {t('settings.connections.mobile.refresh')}
            </Button>
          </div>

          {pairToken && !tokenExpired && (
            <div className="flex flex-col items-center gap-sm" data-testid="mobile-qr">
              <img
                src={pairToken.qrDataUrl}
                alt={t('settings.connections.mobile.qrAlt')}
                className="h-48 w-48 rounded-md border border-surface-border bg-white p-sm"
              />
              <p className="font-label-sm text-on-surface-variant">
                {tVal('settings.connections.mobile.expiresIn', { seconds: remainingSec })}
              </p>
              <code className="font-label-sm text-on-surface-variant break-all">
                {pairToken.lanEndpoint}
              </code>
            </div>
          )}

          {pairToken && tokenExpired && (
            <p
              className="font-body-sm text-on-surface-variant"
              data-testid="mobile-qr-expired"
            >
              {t('settings.connections.mobile.expired')}
            </p>
          )}

          {pairError && (
            <p className="font-body-sm text-error" data-testid="mobile-pair-error">
              {pairError}
            </p>
          )}
        </div>

        {/* Paired devices (registry the gateway writes on shannon/pair). */}
        <div className="space-y-sm border-t border-surface-border pt-md">
          <span className="font-label-md text-on-surface">
            {t('settings.connections.mobile.pairedDevices')}
          </span>
          {pairedDevices.length === 0 ? (
            <p className="font-body-sm text-on-surface-variant">
              {t('settings.connections.mobile.noDevices')}
            </p>
          ) : (
            <ul className="space-y-sm" data-testid="mobile-paired-list">
              {pairedDevices.map((d) => (
                <li
                  key={d.deviceId}
                  className="flex items-center justify-between gap-md"
                  data-testid={`mobile-device-${d.deviceId}`}
                >
                  <div className="flex flex-col">
                    <span className="font-label-sm text-on-surface">
                      {d.label ?? d.deviceId}
                    </span>
                    <code className="font-label-xs text-on-surface-variant">{d.deviceId}</code>
                  </div>
                  <Button variant="secondary" onClick={() => setRevokeTarget(d)}>
                    {t('settings.connections.mobile.revoke')}
                  </Button>
                </li>
              ))}
            </ul>
          )}
        </div>

        <ConfirmDialog
          open={revokeTarget !== null}
          title={t('settings.connections.mobile.revokeConfirmTitle')}
          message={tVal('settings.connections.mobile.revokeConfirmMessage', {
            device: revokeTarget?.label ?? revokeTarget?.deviceId ?? '',
          })}
          confirmLabel={t('settings.connections.mobile.revoke')}
          cancelLabel={t('settings.connections.mobile.cancel')}
          destructive
          busy={revokeBusy}
          busyLabel={t('settings.connections.mobile.revoking')}
          onConfirm={confirmRevoke}
          onCancel={() => setRevokeTarget(null)}
        />
      </CardContent>
    </Card>
  )
}
