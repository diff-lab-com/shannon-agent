import { useState, useEffect, useCallback, useRef } from 'react'
import { useT } from '@/i18n'
import { toast } from 'sonner'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import {
  Select,
  SelectTrigger,
  SelectValue,
  SelectContent,
  SelectItem,
} from '@/components/ui/select'
import * as api from '@/lib/tauri-api'
import { toastError } from '@/lib/errorToast'
import { cn } from '@/lib/utils'

interface VoiceLocalSettingsProps {
  /** Force-disable the card (e.g. when the desktop was built without
   *  the `voice-local` Cargo feature). Renders a "rebuild required"
   *  message instead of the toggles. */
  featureDisabled?: boolean
}

interface DownloadProgress {
  model: string
  progress: number
  done: boolean
  error: string | null
}

/**
 * Advanced-settings card for the local-only STT provider (P2-5e
 * whisper-rs). Surfaces a master switch, a model picker with
 * download / delete controls, a language hint, and an
 * auto-download toggle. Download progress is shown as a
 * <div>-based bar — there's no shared `Progress` component
 * in the UI library.
 *
 * The card is hidden entirely (rather than shown as
 * feature-disabled) when the desktop was built without the
 * `voice-local` Cargo feature; the desktop's `featureFlags`
 * command (out of scope for P2-5e) would gate the parent
 * render.
 */
export function VoiceLocalSettings({ featureDisabled = false }: VoiceLocalSettingsProps) {
  // `t` is a small wrapper that mirrors the `formatMessage({ id },
  // values)` signature used elsewhere in the UI; the Settings
  // cards in this codebase lean on the shorthand so a missing
  // translation is a single grep away.
  const t = useT()
  const [models, setModels] = useState<api.WhisperModelInfo[]>([])
  const [config, setConfig] = useState<api.VoiceLocalConfig | null>(null)
  const [saving, setSaving] = useState(false)
  const [activeDownload, setActiveDownload] = useState<string | null>(null)
  const [downloadProgress, setDownloadProgress] = useState<DownloadProgress | null>(null)
  const unlistenRef = useRef<UnlistenFn | null>(null)

  const refresh = useCallback(async () => {
    try {
      const [cfg, m] = await Promise.all([
        api.getVoiceLocalConfig(),
        api.listWhisperModels(),
      ])
      setConfig(cfg)
      setModels(m)
    } catch (e) {
      // Silent — the card is best-effort. A toast here would
      // pile up on every Settings open.
      console.error('voice local: refresh failed', e)
    }
  }, [])

  useEffect(() => {
    void refresh()
    return () => {
      unlistenRef.current?.()
    }
  }, [refresh])

  // Subscribe to the download progress event ONCE on mount.
  useEffect(() => {
    let cancelled = false
    void (async () => {
      const un = await listen<{
        model: string
        bytes: number | null
        total: number | null
        progress: number
        done: boolean
        error: string | null
      }>('voice:model-download-progress', (event) => {
        if (cancelled) return
        const p = event.payload
        setDownloadProgress({
          model: p.model,
          progress: p.progress,
          done: p.done,
          error: p.error,
        })
        if (p.done) {
          // Re-query the catalog so the card flips to "downloaded"
          // without a manual refresh.
          void refresh()
          if (p.error) {
            toastError(t('settings.voiceLocal.downloadFailed'), p.error)
          } else {
            toast.success(t('settings.voiceLocal.downloadComplete', { model: p.model }))
          }
        }
      })
      if (cancelled) {
        un()
      } else {
        unlistenRef.current = un
      }
    })()
    return () => {
      cancelled = true
      unlistenRef.current?.()
    }
  }, [refresh, t])

  const handleSave = async (next: Partial<api.VoiceLocalConfig>) => {
    if (!config) return
    setSaving(true)
    try {
      const merged: api.VoiceLocalConfig = { ...config, ...next }
      await api.saveVoiceLocalConfig(merged)
      setConfig(merged)
      toast.success(t('settings.voiceLocal.saved'))
    } catch (e) {
      toastError(t('settings.voiceLocal.saveFailed'), e)
    }
    setSaving(false)
  }

  const handleDownload = async (model: string) => {
    setActiveDownload(model)
    setDownloadProgress({ model, progress: 0, done: false, error: null })
    try {
      await api.downloadWhisperModel(model)
    } catch (e) {
      setDownloadProgress({ model, progress: 0, done: true, error: String(e) })
      setActiveDownload(null)
    }
  }

  const handleDelete = async (model: string) => {
    try {
      await api.deleteWhisperModel(model)
      await refresh()
      toast.success(t('settings.voiceLocal.deleted', { model }))
    } catch (e) {
      toastError(t('settings.voiceLocal.deleteFailed'), e)
    }
  }

  if (featureDisabled) {
    return (
      <div className="bg-surface-container-lowest p-lg rounded-xl shadow-sm border border-outline-variant/30 group hover:shadow-md transition-shadow">
        <div className="flex items-center gap-md mb-md">
          <div className="p-2 bg-tertiary/10 rounded-lg text-tertiary flex items-center justify-center">
            <span className="material-symbols-outlined">offline_bolt</span>
          </div>
          <h3 className="font-headline-md text-[24px] font-bold text-on-surface">
            {t('settings.voiceLocal.title')}
          </h3>
        </div>
        <p className="text-on-surface-variant text-body-sm">
          {t('settings.voiceLocal.featureDisabled')}
        </p>
      </div>
    )
  }

  return (
    <div className="bg-surface-container-lowest p-lg rounded-xl shadow-sm border border-outline-variant/30 group hover:shadow-md transition-shadow">
      <div className="flex items-center gap-md mb-md">
        <div className="p-2 bg-primary/10 rounded-lg text-primary flex items-center justify-center">
          <span className="material-symbols-outlined">offline_bolt</span>
        </div>
        <h3 className="font-headline-md text-[24px] font-bold text-on-surface">
          {t('settings.voiceLocal.title')}
        </h3>
        <span
          className={cn(
            "ml-auto px-sm py-[2px] rounded-full text-label-xs font-bold",
            config?.enabled
              ? 'bg-primary-container text-on-primary-container'
              : 'bg-surface-container-high text-on-surface-variant',
          )}
        >
          {config?.enabled ? t('settings.voiceLocal.on') : t('settings.voiceLocal.off')}
        </span>
      </div>
      <p className="text-on-surface-variant text-body-sm mb-lg">
        {t('settings.voiceLocal.description')}
      </p>

      <div className="space-y-md">
        <div className="flex items-center justify-between gap-md">
          <div>
            <div className="font-label-md text-[14px] text-on-surface font-semibold mb-1">
              {t('settings.voiceLocal.enable')}
            </div>
            <div className="font-label-sm text-[12px] text-on-surface-variant leading-tight">
              {t('settings.voiceLocal.enableDesc')}
            </div>
          </div>
          <Switch
            checked={config?.enabled ?? false}
            disabled={!config}
            onCheckedChange={(v) => void handleSave({ enabled: v })}
          />
        </div>

        <div>
          <label className="block font-label-sm text-[12px] text-on-surface-variant mb-1">
            {t('settings.voiceLocal.model')}
          </label>
          <div className="space-y-sm">
            {models.map((m) => {
              const isActive = activeDownload === m.model
              const progress = isActive ? downloadProgress : null
              return (
                <div
                  key={m.model}
                  className="flex items-center gap-md p-sm rounded-lg border border-outline-variant/30 bg-surface-container-low"
                >
                  <div className="flex-1 min-w-0">
                    <div className="font-label-md text-[14px] text-on-surface font-semibold">
                      {m.model}
                    </div>
                    <div className="font-label-sm text-[12px] text-on-surface-variant">
                      {m.downloaded
                        ? t('settings.voiceLocal.modelReady', {
                            size: m.size_bytes
                              ? `${Math.round(m.size_bytes / (1024 * 1024))} MB`
                              : `${m.approx_size_mb} MB`,
                          })
                        : t('settings.voiceLocal.modelNotDownloaded', {
                            size: m.approx_size_mb,
                          })}
                    </div>
                    {isActive && progress && (
                      <div className="mt-xs h-1.5 w-full bg-surface-container-highest rounded overflow-hidden">
                        <div
                          className="h-full bg-primary transition-all"
                          style={{ width: `${Math.round(progress.progress * 100)}%` }}
                        />
                      </div>
                    )}
                  </div>
                  {m.downloaded ? (
                    <Button
                      variant="ghost"
                      size="sm"
                      className="text-error hover:bg-error/10"
                      onClick={() => void handleDelete(m.model)}
                    >
                      {t('settings.voiceLocal.delete')}
                    </Button>
                  ) : (
                    <Button
                      variant="ghost"
                      size="sm"
                      className="text-primary hover:bg-primary/10"
                      disabled={isActive}
                      onClick={() => void handleDownload(m.model)}
                    >
                      {isActive
                        ? t('settings.voiceLocal.downloading', { pct: Math.round((progress?.progress ?? 0) * 100) })
                        : t('settings.voiceLocal.download')}
                    </Button>
                  )}
                </div>
              )
            })}
          </div>
        </div>

        <div>
          <label className="block font-label-sm text-[12px] text-on-surface-variant mb-1">
            {t('settings.voiceLocal.preferredModel')}
          </label>
          <Select
            value={config?.model ?? ''}
            onValueChange={(v) => void handleSave({ model: v || null })}
          >
            <SelectTrigger size="sm" className="w-full" aria-label={t('settings.voiceLocal.preferredModel')}>
              <SelectValue placeholder={t('settings.voiceLocal.preferredModelAuto')} />
            </SelectTrigger>
            <SelectContent>
              {models.map((m) => (
                <SelectItem key={m.model} value={m.model}>
                  {m.model}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>

        <div>
          <label className="block font-label-sm text-[12px] text-on-surface-variant mb-1">
            {t('settings.voiceLocal.language')}
          </label>
          <Input
            type="text"
            value={config?.language ?? ''}
            onChange={(e) => void handleSave({ language: e.target.value || null })}
            placeholder="auto (en, zh, …)"
          />
        </div>

        <div className="flex items-center justify-between gap-md">
          <div>
            <div className="font-label-md text-[14px] text-on-surface font-semibold mb-1">
              {t('settings.voiceLocal.autoDownload')}
            </div>
            <div className="font-label-sm text-[12px] text-on-surface-variant leading-tight">
              {t('settings.voiceLocal.autoDownloadDesc')}
            </div>
          </div>
          <Switch
            checked={config?.auto_download ?? true}
            disabled={!config}
            onCheckedChange={(v) => void handleSave({ auto_download: v })}
          />
        </div>

        {saving && (
          <div className="text-label-xs text-on-surface-variant">
            {t('settings.voiceLocal.saving')}
          </div>
        )}
      </div>
    </div>
  )
}
