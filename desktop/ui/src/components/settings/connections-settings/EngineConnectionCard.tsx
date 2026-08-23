import { useState } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'

import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { toastError } from '@/lib/errorToast'
import * as api from '@/lib/tauri-api'
import type { GatewayConfig } from '@/types'

interface EngineConnectionCardProps {
  config: GatewayConfig
  engineDraft: { wsUrl: string; httpBaseUrl: string }
  onEngineDraftChange: (next: { wsUrl: string; httpBaseUrl: string }) => void
  onConfigChange: (next: GatewayConfig) => void
}

export function EngineConnectionCard({
  config,
  engineDraft,
  onEngineDraftChange,
  onConfigChange,
}: EngineConnectionCardProps) {
  const intl = useIntl()
  const t = (id: string): string => intl.formatMessage({ id })
  const [savingEngine, setSavingEngine] = useState(false)

  async function saveEngine(): Promise<void> {
    const next: GatewayConfig = {
      ...config,
      engine: { ...config.engine, ...engineDraft },
    }
    setSavingEngine(true)
    try {
      const written = await api.gatewayWriteConfig(next)
      onConfigChange(written)
      toast.success(t('settings.connections.engineSaved'))
    } catch (e) {
      toastError('gateway config: write failed', e)
    } finally {
      setSavingEngine(false)
    }
  }

  return (
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
            onChange={(e) => onEngineDraftChange({ ...engineDraft, wsUrl: e.target.value })}
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
            onChange={(e) => onEngineDraftChange({ ...engineDraft, httpBaseUrl: e.target.value })}
            spellCheck={false}
          />
        </div>
        <Button onClick={saveEngine} disabled={savingEngine}>
          {t('settings.connections.saveEngine')}
        </Button>
      </CardContent>
    </Card>
  )
}
