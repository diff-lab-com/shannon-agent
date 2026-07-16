import { useState, useEffect } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectTrigger,
  SelectValue,
  SelectContent,
  SelectItem,
} from '@/components/ui/select'
import * as api from '@/lib/tauri-api'
import { toastError } from '@/lib/errorToast'
import type { SttConfig } from '@/types'

const PROVIDERS = ['groq', 'openai', 'custom'] as const
type SttProvider = (typeof PROVIDERS)[number]

/** Canonical Whisper model id per built-in preset. `custom` has none. */
const DEFAULT_MODEL: Record<SttProvider, string> = {
  groq: 'whisper-large-v3',
  openai: 'whisper-1',
  custom: '',
}

/**
 * Advanced-settings card for cloud speech-to-text (D4 voice input). Configures
 * the provider (Groq / OpenAI / custom), API key, model, and optional base
 * URL. The key is persisted server-side by `save_stt_config`; the field shows
 * a placeholder when a key is already stored so the secret never round-trips
 * to the webview.
 */
export function VoiceSttSettings() {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const [provider, setProvider] = useState<SttProvider>('groq')
  const [apiKey, setApiKey] = useState('')
  const [model, setModel] = useState(DEFAULT_MODEL.groq)
  const [baseUrl, setBaseUrl] = useState('')
  const [keyConfigured, setKeyConfigured] = useState(false)
  const [saving, setSaving] = useState(false)

  useEffect(() => {
    let active = true
    api.getSttConfig()
      .then((cfg) => {
        if (!active || !cfg) return
        const p = (cfg.provider as SttProvider | null) ?? 'groq'
        if (PROVIDERS.includes(p)) setProvider(p)
        setModel(cfg.model ?? DEFAULT_MODEL[p] ?? '')
        setBaseUrl(cfg.base_url ?? '')
        setKeyConfigured(!!cfg.api_key)
      })
      .catch(() => { /* not configured yet — leave defaults */ })
    return () => { active = false }
  }, [])

  const handleProviderChange = (next: string | null) => {
    if (!next || !PROVIDERS.includes(next as SttProvider)) return
    const p = next as SttProvider
    setProvider(p)
    // Reset the model to the preset default for the chosen provider.
    setModel(DEFAULT_MODEL[p])
  }

  const handleSave = async () => {
    setSaving(true)
    try {
      const config: SttConfig = {
        provider,
        // Keep the stored key when the field is left untouched (blank).
        api_key: apiKey.trim() || (keyConfigured ? '***' : null),
        base_url: provider === 'custom' ? baseUrl.trim() || null : null,
        model: model.trim() || null,
      }
      await api.saveSttConfig(config)
      setKeyConfigured(true)
      setApiKey('')
      toast.success(t('settings.voice.saved'))
    } catch (e) {
      toastError(t('settings.voice.saveFailed'), e)
    }
    setSaving(false)
  }

  return (
    <div className="bg-surface-container-lowest p-lg rounded-xl shadow-sm border border-outline-variant/30 group hover:shadow-md transition-shadow">
      <div className="flex items-center gap-md mb-md">
        <div className="p-2 bg-primary/10 rounded-lg text-primary flex items-center justify-center">
          <span className="material-symbols-outlined">mic</span>
        </div>
        <h3 className="font-headline-md text-[24px] font-bold text-on-surface">{t('settings.voice.title')}</h3>
        <span
          className={`ml-auto px-sm py-[2px] rounded-full text-label-xs font-bold ${
            keyConfigured
              ? 'bg-primary-container text-on-primary-container'
              : 'bg-surface-container-high text-on-surface-variant'
          }`}
        >
          {keyConfigured ? t('settings.voice.configured') : t('settings.voice.notConfigured')}
        </span>
      </div>
      <p className="text-on-surface-variant text-body-sm mb-lg">{t('settings.voice.description')}</p>

      <div className="space-y-md">
        <div>
          <label className="block font-label-sm text-[12px] text-on-surface-variant mb-1">{t('settings.voice.provider')}</label>
          <Select value={provider} onValueChange={handleProviderChange}>
            <SelectTrigger size="sm" className="w-full" aria-label={t('settings.voice.provider')}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {PROVIDERS.map((p) => (
                <SelectItem key={p} value={p}>{t(`settings.voice.provider.${p}`)}</SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>

        <div>
          <label className="block font-label-sm text-[12px] text-on-surface-variant mb-1">{t('settings.voice.apiKey')}</label>
          <Input
            type="password"
            value={apiKey}
            onChange={(e) => setApiKey(e.target.value)}
            placeholder={keyConfigured ? '••••••••' : t('settings.voice.apiKeyPlaceholder')}
            autoComplete="off"
          />
        </div>

        <div>
          <label className="block font-label-sm text-[12px] text-on-surface-variant mb-1">{t('settings.voice.model')}</label>
          <Input
            type="text"
            value={model}
            onChange={(e) => setModel(e.target.value)}
            placeholder={DEFAULT_MODEL[provider] || t('settings.voice.modelPlaceholder')}
          />
        </div>

        {provider === 'custom' && (
          <div>
            <label className="block font-label-sm text-[12px] text-on-surface-variant mb-1">{t('settings.voice.baseUrl')}</label>
            <Input
              type="text"
              value={baseUrl}
              onChange={(e) => setBaseUrl(e.target.value)}
              placeholder="https://your-host/v1"
            />
          </div>
        )}

        <p className="font-label-xs text-[11px] text-on-surface-variant">{t('settings.voice.help')}</p>

        <Button
          className="w-full py-md bg-primary text-on-primary rounded-xl font-label-md font-bold text-[14px] hover:bg-primary/90 cursor-pointer"
          onClick={handleSave}
          disabled={saving}
        >
          {saving ? <span className="material-symbols-outlined animate-spin mr-sm text-[18px]">progress_activity</span> : null}
          {t('settings.voice.save')}
        </Button>
      </div>
    </div>
  )
}
