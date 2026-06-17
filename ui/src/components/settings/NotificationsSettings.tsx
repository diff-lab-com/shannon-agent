import { useEffect, useState } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import * as api from '@/lib/tauri-api'

const TEMPLATES = [
  { value: 'slack', label: 'Slack' },
  { value: 'discord', label: 'Discord' },
  { value: 'feishu', label: 'Feishu (飞书)' },
  { value: 'wechat', label: 'WeChat Work (企业微信)' },
  { value: 'teams', label: 'Microsoft Teams' },
  { value: 'telegram', label: 'Telegram' },
  { value: 'dingtalk', label: 'DingTalk (钉钉)' },
  { value: 'raw', label: 'Raw JSON' },
] as const

function isCustom(t: string): t is `custom:${string}` {
  return t.startsWith('custom:')
}

function templateToSelect(t: string): string {
  if (isCustom(t)) return 'custom'
  if (TEMPLATES.some((tpl) => tpl.value === t)) return t
  return ''
}

export default function NotificationsSettings() {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [clearing, setClearing] = useState(false)
  const [url, setUrl] = useState('')
  const [template, setTemplate] = useState<string>('raw')
  const [customBody, setCustomBody] = useState('')
  const [secret, setSecret] = useState('')
  const [showSecret, setShowSecret] = useState(false)
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
        if (isCustom(dto.template)) setCustomBody(dto.template.slice('custom:'.length))
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

  const selectValue = templateToSelect(template)

  const buildDto = (): api.WebhookConfigDto => {
    const finalTemplate = selectValue === 'custom' ? `custom:${customBody}` : (selectValue || 'raw')
    return {
      url: url.trim(),
      template: finalTemplate,
      secret: secret.trim() ? secret.trim() : null,
      timeout_ms: timeoutMs,
      include_body: includeBody,
    }
  }

  const handleSave = async () => {
    if (!url.trim()) {
      toast.error(t('settings.notifications.error.urlRequired'))
      return
    }
    setSaving(true)
    try {
      await api.saveWebhookConfig(buildDto())
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
      <div className="flex items-center justify-center py-12">
        <span className="material-symbols-outlined text-[32px] text-primary animate-spin">progress_activity</span>
      </div>
    )
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

      <div className="bg-surface-container-lowest p-lg rounded-xl shadow-sm border border-outline-variant/30 space-y-md">
        <div>
          <label htmlFor="webhook-url" className="block font-label-lg text-on-surface mb-sm">
            {t('settings.notifications.url')}
          </label>
          <input
            id="webhook-url"
            type="url"
            value={url}
            onChange={(e) => setUrl(e.target.value)}
            placeholder="https://hooks.slack.com/services/..."
            className="w-full px-md py-sm rounded-md border border-outline bg-surface text-on-surface focus:outline-none focus:border-primary"
          />
        </div>

        <div>
          <label htmlFor="webhook-template" className="block font-label-lg text-on-surface mb-sm">
            {t('settings.notifications.template')}
          </label>
          <select
            id="webhook-template"
            value={selectValue || 'raw'}
            onChange={(e) => {
              const v = e.target.value
              if (v === 'custom') {
                setTemplate('custom:')
              } else {
                setTemplate(v)
              }
            }}
            className="w-full px-md py-sm rounded-md border border-outline bg-surface text-on-surface focus:outline-none focus:border-primary"
          >
            {TEMPLATES.map((tpl) => (
              <option key={tpl.value} value={tpl.value}>
                {tpl.label}
              </option>
            ))}
            <option value="custom">
              {t('settings.notifications.templateCustom')}
            </option>
          </select>
        </div>

        {isCustom(template) && (
          <div>
            <label htmlFor="webhook-custom" className="block font-label-lg text-on-surface mb-sm">
              {t('settings.notifications.customBody')}
            </label>
            <textarea
              id="webhook-custom"
              value={customBody}
              onChange={(e) => setCustomBody(e.target.value)}
              placeholder={'{"text": "{title}: {body}"}'}
              rows={3}
              className="w-full px-md py-sm rounded-md border border-outline bg-surface text-on-surface focus:outline-none focus:border-primary font-mono text-sm"
            />
            <p className="text-on-surface-variant text-xs mt-sm">
              {t('settings.notifications.customBodyHint')}
            </p>
          </div>
        )}

        <div>
          <label htmlFor="webhook-secret" className="block font-label-lg text-on-surface mb-sm">
            {t('settings.notifications.secret')}
          </label>
          <div className="flex gap-sm">
            <input
              id="webhook-secret"
              type={showSecret ? 'text' : 'password'}
              value={secret}
              onChange={(e) => setSecret(e.target.value)}
              placeholder={t('settings.notifications.secretPlaceholder')}
              className="flex-1 px-md py-sm rounded-md border border-outline bg-surface text-on-surface focus:outline-none focus:border-primary"
            />
            <Button
              variant="outline"
              onClick={() => setShowSecret((s) => !s)}
              aria-label={t('settings.notifications.toggleSecret')}
            >
              <span className="material-symbols-outlined">
                {showSecret ? 'visibility_off' : 'visibility'}
              </span>
            </Button>
          </div>
          <p className="text-on-surface-variant text-xs mt-sm">
            {t('settings.notifications.secretHint')}
          </p>
        </div>

        <div className="grid grid-cols-1 md:grid-cols-2 gap-md">
          <div>
            <label htmlFor="webhook-timeout" className="block font-label-lg text-on-surface mb-sm">
              {t('settings.notifications.timeoutMs')}
            </label>
            <input
              id="webhook-timeout"
              type="number"
              min={500}
              step={500}
              value={timeoutMs}
              onChange={(e) => setTimeoutMs(Number(e.target.value) || 5000)}
              className="w-full px-md py-sm rounded-md border border-outline bg-surface text-on-surface focus:outline-none focus:border-primary"
            />
          </div>
          <div className="flex items-end pb-sm">
            <div className="flex items-center gap-md">
              <Switch checked={includeBody} onCheckedChange={setIncludeBody} id="include-body" />
              <label htmlFor="include-body" className="font-body-md text-on-surface cursor-pointer">
                {t('settings.notifications.includeBody')}
              </label>
            </div>
          </div>
        </div>

        <div className="flex gap-sm pt-md">
          <Button onClick={handleSave} disabled={saving}>
            {saving ? t('settings.notifications.saving') : t('settings.notifications.save')}
          </Button>
          <Button variant="outline" onClick={handleClear} disabled={clearing}>
            {clearing ? t('settings.notifications.clearing') : t('settings.notifications.clear')}
          </Button>
        </div>

        <p className="text-on-surface-variant text-xs pt-sm border-t border-outline-variant/30">
          {t('settings.notifications.restartHint')}
        </p>
      </div>
    </div>
  )
}
