import { useEffect, useState } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { toastError } from '@/lib/errorToast'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
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
import OutboundSection from './notifications/OutboundSection'

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
      toastError(t('settings.notifications.error.saveFailed'), e)
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
      toastError(t('settings.notifications.error.clearFailed'), e)
    }
    setClearing(false)
  }

  if (loading) {
    return (
      <div className="flex items-center justify-center py-12" role="status" aria-live="polite">
        <span className="material-symbols-outlined icon-xl text-primary animate-spin" aria-hidden="true">progress_activity</span>
        <span className="sr-only">{t('settings.notifications.loading')}</span>
      </div>
    )
  }

  const presetMeta = PRESET_META[preset]

  return (
    <div className="bg-surface-container-lowest p-lg rounded-xl shadow-sm border border-outline-variant/30 space-y-md">
      <div>
        <h4 className="font-headline-md text-on-surface">{t('settings.notifications.webhook.title')}</h4>
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
                <span className="material-symbols-outlined icon-sm" aria-hidden="true">
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
          aria-describedby="webhook-url-hint webhook-url-status"
          className={`w-full px-md py-sm rounded-md border bg-surface text-on-surface focus-visible:border-primary focus-visible:ring-2 focus-visible:ring-primary/30 focus-visible:ring-2 focus-visible:ring-primary/20 ${
            url.trim() ? (validateWebhookUrl(url.trim()).ok ? 'border-tertiary/50' : 'border-error/50') : 'border-outline'
          }`}
        />
        <div className="mt-xs flex items-center gap-xs">
          <p id="webhook-url-hint" className="text-on-surface-variant font-body-sm flex-1">
            {t(presetMeta.urlHintKey)}
          </p>
          {url.trim() && (
            <span
              id="webhook-url-status"
              className={`text-label-sm font-bold ${validateWebhookUrl(url.trim()).ok ? 'text-tertiary' : 'text-error'}`}
            >
              {validateWebhookUrl(url.trim()).ok ? t('settings.notifications.urlStatus.valid') : t('settings.notifications.urlStatus.invalid')}
            </span>
          )}
        </div>
      </div>

      <div className="flex gap-sm pt-md">
        <Button onClick={handleSave} disabled={saving || !url.trim() || !validateWebhookUrl(url.trim()).ok}>
          {saving ? t('settings.notifications.saving') : t('settings.notifications.save')}
        </Button>
      </div>

      <div className="pt-md mt-sm border-t border-error/20 space-y-sm">
        <p className="font-label-sm text-error font-bold uppercase tracking-wide">
          {t('settings.notifications.dangerZone')}
        </p>
        <p className="text-on-surface-variant font-body-sm">
          {t('settings.notifications.clearDescription')}
        </p>
        <Button
          variant="outline"
          onClick={handleClear}
          disabled={clearing || !url}
          className="border-error/40 text-error hover:bg-error/10 hover:border-error"
        >
          {clearing ? t('settings.notifications.clearing') : t('settings.notifications.clear')}
        </Button>
      </div>
    </div>
  )
}

/** Desktop-notification master switch + Do-Not-Disturb quiet-hours window.
 * Desktop-local: webhooks still deliver while DND suppresses OS popups. */
function DndSection() {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [master, setMaster] = useState(true)
  const [dnd, setDnd] = useState(false)
  const [start, setStart] = useState('22:00')
  const [end, setEnd] = useState('07:00')
  const [onCompleted, setOnCompleted] = useState(true)
  const [onFailed, setOnFailed] = useState(true)

  useEffect(() => {
    let cancelled = false
    api.getNotificationPrefs()
      .then((p) => {
        if (cancelled) return
        setMaster(p.master_enabled)
        setDnd(p.dnd_enabled)
        if (p.dnd_start) setStart(p.dnd_start)
        if (p.dnd_end) setEnd(p.dnd_end)
        setOnCompleted(p.on_completed)
        setOnFailed(p.on_failed)
      })
      .catch((e) => toastError(t('settings.notifications.dnd.loadFailed'), e))
      .finally(() => {
        if (!cancelled) setLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [t])

  const handleSave = async () => {
    setSaving(true)
    try {
      await api.setNotificationPrefs({
        master_enabled: master,
        dnd_enabled: dnd,
        dnd_start: dnd ? start : null,
        dnd_end: dnd ? end : null,
        on_completed: onCompleted,
        on_failed: onFailed,
      })
      toast.success(t('settings.notifications.dnd.saved'))
    } catch (e) {
      toastError(t('settings.notifications.dnd.saveFailed'), e)
    } finally {
      setSaving(false)
    }
  }

  const windowDisabled = loading || !master

  return (
    <section className="rounded-xl border border-outline-variant/30 bg-surface-container-lowest p-lg space-y-md">
      <div className="flex items-center justify-between gap-md">
        <div>
          <div className="font-label-md text-[14px] text-on-surface font-semibold mb-1">
            {t('settings.notifications.dnd.master')}
          </div>
          <div className="font-label-sm text-[12px] text-on-surface-variant leading-tight">
            {t('settings.notifications.dnd.masterDesc')}
          </div>
        </div>
        <Switch
          checked={master}
          onCheckedChange={setMaster}
          disabled={loading}
          aria-label={t('settings.notifications.dnd.master')}
          className="shrink-0"
        />
      </div>

      <div className="pt-xs">
        <div className="font-label-sm text-[12px] uppercase tracking-wide text-on-surface-variant mb-sm">
          {t('settings.notifications.dnd.eventsTitle')}
        </div>
        <div className="space-y-md">
          <div className="flex items-center justify-between gap-md">
            <div>
              <div className="font-label-md text-[14px] text-on-surface font-semibold mb-1">
                {t('settings.notifications.dnd.completed')}
              </div>
              <div className="font-label-sm text-[12px] text-on-surface-variant leading-tight">
                {t('settings.notifications.dnd.completedDesc')}
              </div>
            </div>
            <Switch
              checked={onCompleted}
              onCheckedChange={setOnCompleted}
              disabled={windowDisabled}
              aria-label={t('settings.notifications.dnd.completed')}
              className="shrink-0"
            />
          </div>
          <div className="flex items-center justify-between gap-md">
            <div>
              <div className="font-label-md text-[14px] text-on-surface font-semibold mb-1">
                {t('settings.notifications.dnd.failed')}
              </div>
              <div className="font-label-sm text-[12px] text-on-surface-variant leading-tight">
                {t('settings.notifications.dnd.failedDesc')}
              </div>
            </div>
            <Switch
              checked={onFailed}
              onCheckedChange={setOnFailed}
              disabled={windowDisabled}
              aria-label={t('settings.notifications.dnd.failed')}
              className="shrink-0"
            />
          </div>
        </div>
      </div>

      <div className="flex items-center justify-between gap-md">
        <div>
          <div className="font-label-md text-[14px] text-on-surface font-semibold mb-1">
            {t('settings.notifications.dnd.quietHours')}
          </div>
          <div className="font-label-sm text-[12px] text-on-surface-variant leading-tight">
            {t('settings.notifications.dnd.quietHoursDesc')}
          </div>
        </div>
        <Switch
          checked={dnd}
          onCheckedChange={setDnd}
          disabled={windowDisabled}
          aria-label={t('settings.notifications.dnd.quietHours')}
          className="shrink-0"
        />
      </div>

      {dnd && master && (
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-md">
          <label className="flex flex-col gap-xs">
            <span className="font-label-sm text-[12px] text-on-surface-variant">
              {t('settings.notifications.dnd.start')}
            </span>
            <input
              type="time"
              value={start}
              onChange={(e) => setStart(e.target.value)}
              disabled={loading}
              className="rounded-lg border border-outline-variant bg-surface-container-lowest px-md py-sm font-body-md text-on-surface focus:outline-none focus:ring-2 focus:ring-primary"
            />
          </label>
          <label className="flex flex-col gap-xs">
            <span className="font-label-sm text-[12px] text-on-surface-variant">
              {t('settings.notifications.dnd.end')}
            </span>
            <input
              type="time"
              value={end}
              onChange={(e) => setEnd(e.target.value)}
              disabled={loading}
              className="rounded-lg border border-outline-variant bg-surface-container-lowest px-md py-sm font-body-md text-on-surface focus:outline-none focus:ring-2 focus:ring-primary"
            />
          </label>
          <p className="sm:col-span-2 font-label-sm text-[12px] text-on-surface-variant">
            {t('settings.notifications.dnd.windowHint')}
          </p>
        </div>
      )}

      <div>
        <Button onClick={handleSave} disabled={loading || saving}>
          {saving ? t('settings.notifications.dnd.saving') : t('settings.notifications.dnd.save')}
        </Button>
      </div>
    </section>
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
    const pollId = window.setInterval(() => {
      api
        .getInboundListenerStatus()
        .then((s) => {
          if (!cancelled) setStatus(s)
        })
        .catch((e) => console.warn('getInboundListenerStatus poll error:', e))
    }, 30000)
    return () => {
      cancelled = true
      window.clearInterval(pollId)
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
        <span className="material-symbols-outlined icon-xl text-primary animate-spin" aria-hidden="true">progress_activity</span>
        <span className="sr-only">{t('settings.notifications.loading')}</span>
      </div>
    )
  }

  type Channel = {
    id: 'slack' | 'telegram' | 'email'
    name: string
    icon: string
    configured: boolean
    active: boolean
    comingSoon?: boolean
  }
  const channels: Channel[] = [
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
      comingSoon: true,
    },
  ]

  type HealthState = 'connected' | 'inactive' | 'setup'
  const healthFor = (c: (typeof channels)[number]): HealthState => {
    if (!c.configured) return 'setup'
    return c.active ? 'connected' : 'inactive'
  }

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

      <section className="mt-xl">
        <div className="mb-lg">
          <h3 className="font-headline-md text-on-surface mb-xs">{t('settings.notifications.dnd.sectionTitle')}</h3>
          <p className="text-on-surface-variant font-body-sm">{t('settings.notifications.dnd.sectionDesc')}</p>
        </div>
        <DndSection />
      </section>

      <section className="mt-xl">
        <div className="mb-lg">
          <h3 className="font-headline-md text-on-surface mb-xs">{t('settings.notifications.outbound.sectionTitle')}</h3>
          <p className="text-on-surface-variant font-body-sm">{t('settings.notifications.outbound.sectionDesc')}</p>
        </div>
        <div className="space-y-lg">
          <WebhookSection />
          <OutboundSection />
        </div>
      </section>

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
                onClick={() => { if (!channel.comingSoon) setSelectedChannel(channel.id) }}
                disabled={!!channel.comingSoon}
                className={`p-md rounded-xl border bg-surface-container-lowest transition-colors text-left ${
                  channel.comingSoon
                    ? 'border-outline-variant/20 opacity-60 cursor-not-allowed'
                    : 'border-outline-variant/30 hover:border-primary/50'
                }`}
              >
                <div className="flex items-center justify-between mb-sm">
                  <div className="flex items-center gap-sm">
                    <span className="material-symbols-outlined text-primary">{channel.icon}</span>
                    <span className="font-label-md font-bold text-on-surface">{channel.name}</span>
                  </div>
                  {(() => {
                    if (channel.comingSoon) {
                      return (
                        <span className="inline-flex items-center gap-xs px-xs py-xxs rounded-full bg-surface-container-high text-on-surface-variant font-label-sm" role="status">
                          {t('settings.notifications.channel.comingSoon')}
                        </span>
                      )
                    }
                    const health = healthFor(channel)
                    if (health === 'connected') {
                      return (
                        <span className="inline-flex items-center gap-xs px-xs py-xxs rounded-full bg-tertiary/20 text-tertiary font-label-sm" role="status">
                          <span className="w-1 h-1 rounded-full bg-tertiary animate-pulse" aria-hidden="true" />
                          {t('settings.notifications.wizard.channel.status.connected')}
                        </span>
                      )
                    }
                    if (health === 'inactive') {
                      return (
                        <span className="inline-flex items-center gap-xs px-xs py-xxs rounded-full bg-error/15 text-error font-label-sm" role="status" title={t('settings.notifications.wizard.channel.status.inactive')}>
                          <span className="material-symbols-outlined icon-xs" aria-hidden="true">warning</span>
                          {t('settings.notifications.wizard.channel.status.inactive')}
                        </span>
                      )
                    }
                    return null
                  })()}
                </div>
                {channel.comingSoon ? (
                  <p className="text-on-surface-variant text-sm">
                    {t('settings.notifications.wizard.email.comingSoon')}
                  </p>
                ) : channel.configured ? (
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
          </>
        )}
      </div>
    </div>
  )
}
