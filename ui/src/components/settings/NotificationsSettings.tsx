import { useEffect, useState } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { validateWebhookUrl } from '@/lib/packageValidation'
import * as api from '@/lib/tauri-api'
import SlackWizard from './notifications/SlackWizard'
import TelegramWizard from './notifications/TelegramWizard'
import EmailWizard from './notifications/EmailWizard'

type ChannelType = 'slack' | 'telegram' | 'email' | null

/** Channel preset id — stored as the webhook `template` discriminator. */
type WebhookPreset = 'feishu' | 'dingtalk' | 'wechat' | 'slack' | 'custom'

const PRESET_IDS: WebhookPreset[] = ['feishu', 'dingtalk', 'wechat', 'slack', 'custom']

const PRESET_META: Record<
  WebhookPreset,
  { icon: string; urlPlaceholder: string; urlHintKey: string; labelKey: string }
> = {
  feishu: {
    icon: 'forum',
    urlPlaceholder: 'https://open.feishu.cn/open-apis/bot/v2/hook/send/<token>',
    urlHintKey: 'settings.notifications.preset.urlHint.feishu',
    labelKey: 'settings.notifications.preset.feishu',
  },
  dingtalk: {
    icon: 'notifications',
    urlPlaceholder: 'https://oapi.dingtalk.com/robot/send?access_token=<token>',
    urlHintKey: 'settings.notifications.preset.urlHint.dingtalk',
    labelKey: 'settings.notifications.preset.dingtalk',
  },
  wechat: {
    icon: 'chat',
    urlPlaceholder: 'https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=<key>',
    urlHintKey: 'settings.notifications.preset.urlHint.wechat',
    labelKey: 'settings.notifications.preset.wechat',
  },
  slack: {
    icon: 'tag',
    urlPlaceholder: 'https://hooks.slack.com/services/...',
    urlHintKey: 'settings.notifications.preset.urlHint.slack',
    labelKey: 'settings.notifications.preset.slack',
  },
  custom: {
    icon: 'tune',
    urlPlaceholder: 'https://example.com/webhook',
    urlHintKey: 'settings.notifications.url',
    labelKey: 'settings.notifications.preset.custom',
  },
}

/** Map a stored template string back to its preset id. Unknown values → 'custom'. */
function presetFromTemplate(template: string | undefined): WebhookPreset {
  if (!template) return 'custom'
  if (template.startsWith('custom:')) return 'custom'
  return PRESET_IDS.includes(template as WebhookPreset) ? (template as WebhookPreset) : 'custom'
}

function WebhookSection() {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [clearing, setClearing] = useState(false)
  const [url, setUrl] = useState('')
  const [template, setTemplate] = useState('raw')
  const [preset, setPreset] = useState<WebhookPreset>('custom')
  const [customBody, setCustomBody] = useState('')
  const [secret, setSecret] = useState('')
  const [timeoutMs, setTimeoutMs] = useState(5000)
  const [includeBody, setIncludeBody] = useState(false)

  useEffect(() => {
    let cancelled = false
    api
      .getWebhookConfig()
      .then((dto) => {
        if (cancelled || !dto) return
        setUrl(dto.url)
        setTemplate(dto.template || 'raw')
        setPreset(presetFromTemplate(dto.template))
        if (dto.template?.startsWith('custom:')) setCustomBody(dto.template.slice('custom:'.length))
        setSecret(dto.secret ?? '')
        setTimeoutMs(dto.timeout_ms || 5000)
        setIncludeBody(dto.include_body)
      })
      .catch((e) => console.warn('getWebhookConfig error:', e))
      .finally(() => {
        if (!cancelled) setLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [])

  const handlePresetChange = (next: WebhookPreset) => {
    setPreset(next)
    // Only pre-fill the URL when the user hasn't typed one yet.
    if (!url.trim()) {
      setUrl(PRESET_META[next].urlPlaceholder)
    }
  }

  const handleSave = async () => {
    const trimmed = url.trim()
    if (!trimmed) {
      toast.error(t('settings.notifications.error.urlRequired'))
      return
    }
    const check = validateWebhookUrl(trimmed)
    if (!check.ok) {
      const key =
        check.reason === 'scheme' ? 'settings.notifications.error.urlBadScheme'
        : check.reason === 'private' ? 'settings.notifications.error.urlPrivate'
        : 'settings.notifications.error.urlInvalid'
      toast.error(t(key))
      return
    }
    setSaving(true)
    try {
      // Encode the selected preset as the template discriminator. Preserves the
      // legacy `custom:<body>` shape for the Custom preset.
      const encodedTemplate =
        preset === 'custom'
          ? template.startsWith('custom:')
            ? template
            : `custom:${customBody}`
          : preset
      await api.saveWebhookConfig({
        url: url.trim(),
        template: encodedTemplate,
        secret: secret.trim() || null,
        timeout_ms: timeoutMs,
        include_body: includeBody,
      })
      toast.success(t('settings.notifications.saved'))
    } catch (e) {
      console.warn('saveWebhookConfig error:', e)
      toast.error(t('settings.notifications.error.saveFailed'))
    }
    setSaving(false)
  }

  const handleClear = async () => {
    setClearing(true)
    try {
      await api.clearWebhookConfig()
      setUrl('')
      setTemplate('raw')
      setPreset('custom')
      setCustomBody('')
      setSecret('')
      setTimeoutMs(5000)
      setIncludeBody(false)
      toast.success(t('settings.notifications.cleared'))
    } catch (e) {
      console.warn('clearWebhookConfig error:', e)
      toast.error(t('settings.notifications.error.clearFailed'))
    }
    setClearing(false)
  }

  if (loading) {
    return (
      <div className="flex items-center justify-center py-12" role="status" aria-live="polite">
        <span className="material-symbols-outlined text-[32px] text-primary animate-spin" aria-hidden="true">progress_activity</span>
        <span className="sr-only">{t('settings.notifications.loading')}</span>
      </div>
    )
  }

  const presetMeta = PRESET_META[preset]

  return (
    <div className="bg-surface-container-lowest p-lg rounded-xl shadow-sm border border-outline-variant/30 space-y-md">
      <div>
        <h3 className="font-headline-md text-on-surface">{t('settings.notifications.webhook.title')}</h3>
        <p className="text-on-surface-variant font-body-sm">{t('settings.notifications.webhook.subtitle')}</p>
      </div>

      <div>
        <label
          id="webhook-preset-label"
          className="block font-label-lg text-on-surface mb-sm"
        >
          {t('settings.notifications.preset.label')}
        </label>
        <Select value={preset} onValueChange={(v) => handlePresetChange(v as WebhookPreset)}>
          <SelectTrigger
            aria-labelledby="webhook-preset-label"
            className="w-full border-outline bg-surface text-on-surface"
          >
            <span className="material-symbols-outlined text-[18px] text-primary" aria-hidden="true">
              {presetMeta.icon}
            </span>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {PRESET_IDS.map((id) => (
              <SelectItem key={id} value={id}>
                <span className="material-symbols-outlined text-[16px]" aria-hidden="true">
                  {PRESET_META[id].icon}
                </span>
                <span>{t(PRESET_META[id].labelKey)}</span>
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      <div>
        <label htmlFor="webhook-url" className="block font-label-lg text-on-surface mb-sm">
          {t('settings.notifications.url')}
        </label>
        <input
          id="webhook-url"
          type="url"
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          placeholder={presetMeta.urlPlaceholder}
          aria-describedby="webhook-url-hint"
          className="w-full px-md py-sm rounded-md border border-outline bg-surface text-on-surface focus:outline-none focus:border-primary"
        />
        <p id="webhook-url-hint" className="mt-xs text-on-surface-variant font-body-sm">
          {t(presetMeta.urlHintKey)}
        </p>
      </div>

      <div className="flex gap-sm pt-md">
        <Button onClick={handleSave} disabled={saving}>
          {saving ? t('settings.notifications.saving') : t('settings.notifications.save')}
        </Button>
        <Button variant="outline" onClick={handleClear} disabled={clearing}>
          {clearing ? t('settings.notifications.clearing') : t('settings.notifications.clear')}
        </Button>
      </div>
    </div>
  )
}

export default function NotificationsSettings() {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  const [loading, setLoading] = useState(true)
  const [selectedChannel, setSelectedChannel] = useState<ChannelType>(null)
  const [inboundConfig, setInboundConfig] = useState<api.InboundConfigDto | null>(null)
  const [status, setStatus] = useState<api.InboundListenerStatus | null>(null)

  useEffect(() => {
    let cancelled = false
    api
      .getInboundConfig()
      .then((cfg) => {
        if (cancelled) return
        setInboundConfig(cfg)
      })
      .catch((e) => console.warn('getInboundConfig error:', e))
      .finally(() => {
        if (!cancelled) setLoading(false)
      })
    api
      .getInboundListenerStatus()
      .then((s) => {
        if (!cancelled) setStatus(s)
      })
      .catch((e) => console.warn('getInboundListenerStatus error:', e))
    return () => {
      cancelled = true
    }
  }, [])

  const handleSaveInbound = async (dto: api.SlackInboundDto | api.TelegramInboundDto) => {
    const config: api.InboundConfigDto = {
      slack: 'bot_token' in dto ? dto as api.SlackInboundDto : inboundConfig?.slack || null,
      telegram: 'allowed_chats' in dto ? dto as api.TelegramInboundDto : inboundConfig?.telegram || null,
    }
    await api.saveInboundConfig(config)
    setInboundConfig(config)
    const updated = await api.getInboundListenerStatus()
    setStatus(updated)
    setSelectedChannel(null)
  }

  if (loading) {
    return (
      <div className="flex items-center justify-center py-12" role="status" aria-live="polite">
        <span className="material-symbols-outlined text-[32px] text-primary animate-spin" aria-hidden="true">progress_activity</span>
        <span className="sr-only">{t('settings.notifications.loading')}</span>
      </div>
    )
  }

  const channels = [
    {
      id: 'slack' as const,
      name: 'Slack',
      icon: 'tag',
      configured: !!inboundConfig?.slack,
      active: status?.slack_running || false,
    },
    {
      id: 'telegram' as const,
      name: 'Telegram',
      icon: 'send',
      configured: !!inboundConfig?.telegram,
      active: status?.telegram_running || false,
    },
    {
      id: 'email' as const,
      name: 'Email',
      icon: 'email',
      configured: false,
      active: false,
    },
  ]

  return (
    <div className="pb-xl">
      <div className="mb-xl">
        <h2 className="font-headline-lg text-headline-lg text-on-surface mb-sm">
          {t('settings.notifications.title')}
        </h2>
        <p className="text-on-surface-variant font-body-md">
          {t('settings.notifications.subtitle')}
        </p>
      </div>

      <WebhookSection />

      <div className="mt-xl">
        <div className="mb-lg">
          <h3 className="font-headline-md text-on-surface mb-xs">{t('settings.notifications.inbound.title')}</h3>
          <p className="text-on-surface-variant font-body-sm">{t('settings.notifications.inbound.subtitle')}</p>
        </div>

        {!selectedChannel ? (
          <div className="grid grid-cols-1 md:grid-cols-3 gap-md">
            {channels.map((channel) => (
              <button
                key={channel.id}
                onClick={() => setSelectedChannel(channel.id)}
                className="p-md rounded-xl border border-outline-variant/30 bg-surface-container-lowest hover:border-primary/50 transition-colors text-left"
              >
                <div className="flex items-center justify-between mb-sm">
                  <div className="flex items-center gap-sm">
                    <span className="material-symbols-outlined text-primary">tag</span>
                    <span className="font-label-md font-bold text-on-surface">{channel.name}</span>
                  </div>
                  {channel.configured && channel.active && (
                    <span className="inline-flex items-center gap-xs px-xs py-xxs rounded-full bg-tertiary/20 text-tertiary font-label-sm">
                      <span className="w-1 h-1 rounded-full bg-tertiary animate-pulse" />
                      {t('settings.notifications.wizard.channel.status.connected')}
                    </span>
                  )}
                </div>
                {channel.configured ? (
                  <p className="text-on-surface-variant text-sm">
                    {t('settings.notifications.wizard.channel.status.configured')}
                  </p>
                ) : (
                  <p className="text-primary text-sm font-body-md">
                    + {t('settings.notifications.wizard.channel.status.setup')}
                  </p>
                )}
              </button>
            ))}
          </div>
        ) : (
          <>
            {selectedChannel === 'slack' && (
              <SlackWizard
                config={inboundConfig?.slack || null}
                onSave={handleSaveInbound}
                onCancel={() => setSelectedChannel(null)}
              />
            )}
            {selectedChannel === 'telegram' && (
              <TelegramWizard
                config={inboundConfig?.telegram || null}
                onSave={handleSaveInbound}
                onCancel={() => setSelectedChannel(null)}
              />
            )}
            {selectedChannel === 'email' && (
              <EmailWizard
                onSave={async () => {
                  // Email not implemented in backend yet
                  toast.info(t('settings.notifications.wizard.email.comingSoon'))
                }}
                onCancel={() => setSelectedChannel(null)}
              />
            )}
          </>
        )}
      </div>
    </div>
  )
}
