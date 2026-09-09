import { useCallback, useEffect, useRef, useState } from 'react'
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import * as api from '@/lib/tauri-api'
import type {
  PreviewDetectResponse,
  PreviewLogLine,
  PreviewStatusResponse,
} from '@/lib/tauri-api'

// Sandbox baseline follows HtmlRenderer (`allow-scripts`) extended with the
// minimal set real dev servers need: forms (login/dev flows) and modals
// (alert/confirm from demo code). `allow-same-origin` is deliberately NOT
// granted — the iframe keeps an opaque origin, so it cannot reach the
// parent app's DOM/storage even though it is a real network page.
const LIVE_SANDBOX = 'allow-scripts allow-forms allow-modals'

// How long the frame may stay on "loading" before we surface a load error.
const LOAD_TIMEOUT_MS = 20_000

type Phase = 'idle' | 'starting' | 'running'

export function LivePreview() {
  const intl = useIntl()
  const t = useCallback((id: string, values?: Record<string, string>) =>
    intl.formatMessage({ id }, values), [intl])

  const [phase, setPhase] = useState<Phase>('idle')
  const [url, setUrl] = useState<string | null>(null)
  const [detect, setDetect] = useState<PreviewDetectResponse | null>(null)
  const [loadState, setLoadState] = useState<'loading' | 'loaded' | 'error'>('loading')
  const [frameKey, setFrameKey] = useState(0)
  const [error, setError] = useState<string | null>(null)
  const [logs, setLogs] = useState<PreviewLogLine[]>([])
  const [showLogs, setShowLogs] = useState(false)
  const mounted = useRef(true)

  useEffect(() => {
    mounted.current = true
    return () => { mounted.current = false }
  }, [])

  const refreshLogs = useCallback(async () => {
    try {
      const lines = await api.previewLogs(30)
      if (mounted.current) setLogs(lines)
    } catch { /* logs are best-effort */ }
  }, [])

  // The preview manager is a global singleton on the backend, so the
  // running state survives session switches (and panel remounts) — this
  // mount-time sync is all the persistence the panel needs.
  const sync = useCallback(async () => {
    setError(null)
    const status: PreviewStatusResponse = await api.previewStatus()
    if (!mounted.current) return
    if (status.running && status.url) {
      setUrl(status.url)
      setLoadState('loading')
      setPhase('running')
      void refreshLogs()
      return
    }
    setPhase('idle')
    setUrl(null)
    setDetect(await api.previewDetect())
  }, [refreshLogs])

  useEffect(() => {
    sync().catch((e) => {
      if (mounted.current) setError(String(e))
    })
    return () => { mounted.current = false }
  }, [sync])

  const handleStart = async () => {
    setError(null)
    setPhase('starting')
    try {
      const { url: started } = await api.previewStart()
      if (!mounted.current) return
      setUrl(started)
      setLoadState('loading')
      setPhase('running')
      void refreshLogs()
    } catch (e) {
      if (!mounted.current) return
      setPhase('idle')
      setError(String(e))
    }
  }

  const handleStop = async () => {
    try {
      await api.previewStop()
    } catch (e) {
      setError(String(e))
    }
    if (!mounted.current) return
    setPhase('idle')
    setUrl(null)
    setDetect(await api.previewDetect().catch(() => null))
  }

  const handleReload = () => {
    setLoadState('loading')
    setFrameKey(k => k + 1)
    void refreshLogs()
  }

  // Load watchdog: a dev server that died between start and frame load
  // never fires onLoad — surface a retry state instead of an eternal spinner.
  useEffect(() => {
    if (phase !== 'running' || loadState !== 'loading') return
    const timer = setTimeout(() => {
      setLoadState(prev => (prev === 'loading' ? 'error' : prev))
    }, LOAD_TIMEOUT_MS)
    return () => clearTimeout(timer)
  }, [phase, loadState, frameKey])

  const statusText = phase === 'running'
    ? t('chat.artifact.live.status.running', { url: url ?? '' })
    : phase === 'starting'
      ? t('chat.artifact.live.starting')
      : t('chat.artifact.live.status.stopped')

  return (
    <div className="flex flex-col h-full min-h-0" aria-label={t('chat.artifact.live.aria')}>
      <div aria-live="polite" className="sr-only">{statusText}</div>

      {phase === 'running' && url ? (
        <>
          <div className="flex items-center gap-xs px-md py-xs border-b border-outline-variant/10">
            <span className="w-2 h-2 rounded-full bg-primary shrink-0" aria-hidden="true" />
            <input
              type="text"
              readOnly
              value={url}
              aria-label={t('chat.artifact.live.address.aria')}
              className="flex-1 min-w-0 font-body-sm font-mono text-on-surface-variant bg-surface-container-high rounded px-sm py-1 outline-none"
            />
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={handleReload}
              aria-label={t('chat.artifact.live.refresh.aria')}
              title={t('chat.artifact.live.refresh.aria')}
              className="text-on-surface-variant hover:bg-surface-container hover:text-on-surface"
            >
              <span className="material-symbols-outlined icon-sm">refresh</span>
            </Button>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={handleStop}
              aria-label={t('chat.artifact.live.stop.aria')}
              title={t('chat.artifact.live.stop.aria')}
              className="text-on-surface-variant hover:bg-surface-container hover:text-on-surface"
            >
              <span className="material-symbols-outlined icon-sm">stop_circle</span>
              {t('chat.artifact.live.stop')}
            </Button>
          </div>

          <div className="relative flex-1 min-h-0">
            <iframe
              key={frameKey}
              title={t('chat.artifact.live.frame.title')}
              src={url}
              sandbox={LIVE_SANDBOX}
              onLoad={() => setLoadState('loaded')}
              className="w-full h-full bg-white border-0"
            />
            {loadState === 'loading' && (
              <div className="absolute inset-0 flex items-center justify-center bg-surface-container-lowest/80">
                <span className="animate-spin material-symbols-outlined text-primary" aria-hidden="true">progress_activity</span>
              </div>
            )}
            {loadState === 'error' && (
              <div className="absolute inset-0 flex flex-col items-center justify-center gap-sm p-md text-center">
                <span className="material-symbols-outlined icon-lg text-on-surface-variant" aria-hidden="true">cloud_off</span>
                <p className="font-body-md text-on-surface-variant max-w-64">{t('chat.artifact.live.loadError')}</p>
                <Button type="button" variant="outline" size="sm" onClick={handleReload}>
                  {t('chat.artifact.live.retry')}
                </Button>
              </div>
            )}
          </div>
        </>
      ) : (
        <div className="flex-1 flex flex-col items-center justify-center gap-md p-lg text-center">
          <span className="material-symbols-outlined icon-lg text-on-surface-variant" aria-hidden="true">deployed_code</span>
          {error ? (
            <>
              <p className="font-body-md text-error max-w-72">{error}</p>
              <Button type="button" variant="outline" size="sm" onClick={() => { void sync() }}>
                {t('chat.artifact.live.retry')}
              </Button>
            </>
          ) : detect?.devServer ? (
            <>
              <p className="font-body-sm text-on-surface-variant">
                {t('chat.artifact.live.detected', { command: detect.devServer.command })}
              </p>
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={handleStart}
                disabled={phase === 'starting'}
                aria-label={t('chat.artifact.live.start.aria')}
              >
                <span className="material-symbols-outlined icon-sm">play_arrow</span>
                {phase === 'starting' ? t('chat.artifact.live.starting') : t('chat.artifact.live.start')}
              </Button>
            </>
          ) : (
            <p className="font-body-md text-on-surface-variant max-w-72">
              {t('chat.artifact.live.notDetected')}
            </p>
          )}
        </div>
      )}

      {phase === 'running' && (
        <div className="border-t border-outline-variant/10">
          <button
            type="button"
            onClick={() => setShowLogs(s => !s)}
            aria-expanded={showLogs}
            aria-label={t('chat.artifact.live.logs.aria')}
            className="w-full flex items-center gap-xs px-md py-xs font-label-xs text-on-surface-variant hover:bg-surface-container"
          >
            <span className="material-symbols-outlined icon-sm" aria-hidden="true">terminal</span>
            {t('chat.artifact.live.logs')}
          </button>
          {showLogs && (
            <div className="max-h-40 overflow-auto px-md pb-sm font-mono text-xs text-on-surface-variant whitespace-pre-wrap break-all">
              {logs.length === 0
                ? <p className="py-xs">{t('chat.artifact.live.logsEmpty')}</p>
                : logs.map((line, i) => (
                  <div key={`${line.tsMs}-${i}`} className="py-[1px]">
                    <span className="opacity-60">{line.stream}</span>{' '}
                    {line.text}
                  </div>
                ))}
            </div>
          )}
        </div>
      )}
    </div>
  )
}
