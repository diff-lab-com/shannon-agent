import { useEffect, useState } from 'react'
import LoadingState from '@/components/ui/loading-state'
import { useT } from '@/i18n'
import { toast } from 'sonner'
import { toastError } from '@/lib/errorToast'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import { useNotification } from '@/hooks/useNotification'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { validateWebhookUrl } from '@/lib/packageValidation'
import * as api from '@/lib/tauri-api'
import { cn } from '@/lib/utils'
/** Channel preset id — stored as the webhook `template` discriminator. */
type WebhookPreset = 'feishu' | 'dingtalk' | 'wechat' | 'slack' | 'discord' | 'custom'

const PRESET_IDS: WebhookPreset[] = ['feishu', 'dingtalk', 'wechat', 'slack', 'discord', 'custom']

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
  discord: {
    icon: 'forum',
    urlPlaceholder: 'https://discord.com/api/webhooks/<id>/<token>',
    urlHintKey: 'settings.notifications.preset.urlHint.discord',
    labelKey: 'settings.notifications.preset.discord',
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

/** Form-state snapshot of the webhook config as it is currently PERSISTED.
 * R2-P1-7: `test_webhook` re-loads the saved config from disk, so the test
 * verdict only matches what the form shows when the two agree — the diff
 * below is the "dirty" gate for the test button. */
interface WebhookSnapshot {
  url: string
  preset: WebhookPreset
  customBody: string
  secret: string
  timeoutMs: number
  includeBody: boolean
}

const WEBHOOK_DEFAULTS: WebhookSnapshot = {
  url: '',
  preset: 'custom',
  customBody: '',
  secret: '',
  timeoutMs: 5000,
  includeBody: false,
}

function webhookSnapshotFromDto(dto: api.WebhookConfigDto): WebhookSnapshot {
  return {
    url: dto.url,
    preset: presetFromTemplate(dto.template),
    customBody: dto.template?.startsWith('custom:') ? dto.template.slice('custom:'.length) : '',
    secret: dto.secret ?? '',
    timeoutMs: dto.timeout_ms || 5000,
    includeBody: dto.include_body,
  }
}

function webhookSnapshotDiffers(a: WebhookSnapshot, b: WebhookSnapshot): boolean {
  return (
    a.url !== b.url ||
    a.preset !== b.preset ||
    a.customBody !== b.customBody ||
    a.secret !== b.secret ||
    a.timeoutMs !== b.timeoutMs ||
    a.includeBody !== b.includeBody
  )
}

function WebhookSection({ onSaved }: { onSaved?: () => void } = {}) {
  const t = useT()

  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [clearing, setClearing] = useState(false)
  // P1-7: one-shot test send against the configured webhook.
  const [testingWebhook, setTestingWebhook] = useState(false)
  const [url, setUrl] = useState('')
  const [preset, setPreset] = useState<WebhookPreset>('custom')
  const [customBody, setCustomBody] = useState('')
  const [secret, setSecret] = useState('')
  const [timeoutMs, setTimeoutMs] = useState(5000)
  const [includeBody, setIncludeBody] = useState(false)
  // R2-P1-7: what the backend currently has persisted (`null` = nothing
  // saved). The test button is gated on the form matching this snapshot.
  const [saved, setSaved] = useState<WebhookSnapshot | null>(null)
  // R2-P1-7: timeout input that is not a positive number — surfaced inline
  // instead of being silently swallowed (the last valid value still sticks).
  const [timeoutInvalid, setTimeoutInvalid] = useState(false)

  useEffect(() => {
    let cancelled = false
    api
      .getWebhookConfig()
      .then((dto) => {
        if (cancelled) return
        if (!dto) {
          setSaved(null)
          return
        }
        setUrl(dto.url)
        setPreset(presetFromTemplate(dto.template))
        if (dto.template?.startsWith('custom:')) setCustomBody(dto.template.slice('custom:'.length))
        setSecret(dto.secret ?? '')
        setTimeoutMs(dto.timeout_ms || 5000)
        setIncludeBody(dto.include_body)
        setSaved(webhookSnapshotFromDto(dto))
      })
      .catch((e) => console.warn('getWebhookConfig error:', e))
      .finally(() => {
        if (!cancelled) setLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [])

  // R2-P1-7: true whenever the form no longer matches the persisted config —
  // a webhook test fired in this state would exercise the OLD config, so the
  // button is disabled and an inline hint asks for a save first.
  const webhookDirty = webhookSnapshotDiffers(
    { url, preset, customBody, secret, timeoutMs, includeBody },
    saved ?? WEBHOOK_DEFAULTS,
  )

  const handlePresetChange = (next: WebhookPreset) => {
    setPreset(next)
    // R2-P1-7: the preset's example URL (`…/<token>`) is NOT pre-filled into
    // `url` anymore — it lives only in the input's placeholder attribute, so
    // a placeholder can never be saved as a real URL (it used to pass
    // validation once percent-encoded and made every save/test verdict
    // meaningless).
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
    // P1-15: a Custom preset without a body would persist an empty payload
    // and silently wipe the stored template — block the save outright.
    if (preset === 'custom' && customBody.trim() === '') {
      toast.error(t('settings.notifications.error.customBodyRequired'))
      return
    }
    setSaving(true)
    try {
      // Encode the selected preset as the template discriminator. The custom
      // body now comes from its textarea (the guard above guarantees a
      // non-empty body, so saving can no longer wipe the stored template).
      const encodedTemplate = preset === 'custom' ? `custom:${customBody}` : preset
      await api.saveWebhookConfig({
        url: url.trim(),
        template: encodedTemplate,
        secret: secret.trim() || null,
        timeout_ms: timeoutMs,
        include_body: includeBody,
      })
      // R2-P1-7: re-baseline the persisted snapshot so the test button
      // lights up again immediately (the form now matches disk).
      setSaved({
        url: url.trim(),
        preset,
        customBody: preset === 'custom' ? customBody : '',
        secret: secret.trim(),
        timeoutMs,
        includeBody,
      })
      toast.success(t('settings.notifications.saved'))
      onSaved?.()
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
      setPreset('custom')
      setCustomBody('')
      setSecret('')
      setTimeoutMs(5000)
      setIncludeBody(false)
      setSaved(null)
      setTimeoutInvalid(false)
      toast.success(t('settings.notifications.cleared'))
      onSaved?.()
    } catch (e) {
      toastError(t('settings.notifications.error.clearFailed'), e)
    }
    setClearing(false)
  }

  // P1-7: the timeout had state that participated in saves but no control —
  // expose it. Non-positive / non-numeric input keeps the last valid value
  // (so the saved `timeout_ms` can never degrade to 0) and is now surfaced
  // inline instead of being silently ignored (R2-P1-7).
  const handleTimeoutChange = (raw: string) => {
    const n = Number.parseInt(raw, 10)
    if (Number.isFinite(n) && n > 0) {
      setTimeoutMs(n)
      setTimeoutInvalid(false)
    } else {
      setTimeoutInvalid(true)
    }
  }

  // P1-7: send a one-shot test payload through the saved webhook config
  // (preset template + secret + timeout) and surface the verdict.
  const handleTestWebhook = async () => {
    setTestingWebhook(true)
    try {
      const result = await api.testWebhook(
        t('settings.notifications.webhookTest.title'),
        t('settings.notifications.webhookTest.body'),
      )
      if (result.success) {
        toast.success(t('settings.notifications.webhookTest.success', { status: result.status ?? '?' }))
      } else {
        toast.error(t('settings.notifications.webhookTest.failedWithReason', { reason: result.detail }))
      }
    } catch (e) {
      // Command-level rejection (e.g. nothing configured yet).
      toastError(t('settings.notifications.webhookTest.failed'), e)
    }
    setTestingWebhook(false)
  }

  if (loading) {
    return (
      <LoadingState size="lg" label={t('settings.notifications.loading')} />
    )
  }

  const presetMeta = PRESET_META[preset]

  return (
    <div className="bg-surface-container-lowest p-lg rounded-xl shadow-e1 border border-outline-variant/30 space-y-md">
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
            <span className="material-symbols-outlined icon-md text-primary" aria-hidden="true">
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
          className={cn(
            "w-full px-md py-sm rounded-md border bg-surface text-on-surface focus-visible:border-primary focus-visible:ring-2 focus-visible:ring-primary/30 focus-visible:ring-2 focus-visible:ring-primary/20",
            url.trim()
              ? (validateWebhookUrl(url.trim()).ok ? 'border-tertiary/50' : 'border-error/50')
              : 'border-outline',
          )}
        />
        <div className="mt-xs flex items-center gap-xs">
          <p id="webhook-url-hint" className="text-on-surface-variant font-body-sm flex-1">
            {t(presetMeta.urlHintKey)}
          </p>
          {/* Audit D9 — the loopback listener exposes the same secret for
              INBOUND routine triggers (POST /api/routines/:id/trigger, HMAC).
              Surface the endpoint so automation authors can wire GitHub /
              IM webhooks to it. */}
          <p className="text-on-surface-variant font-body-sm mt-sm flex items-start gap-xs">
            <span className="material-symbols-outlined icon-sm text-primary" aria-hidden="true">sync_alt</span>
            <span>
              {t('settings.notifications.inboundHint', { endpoint: 'POST http://127.0.0.1:33420/api/routines/:id/trigger' })}
            </span>
          </p>
          {url.trim() && (
            <span
              id="webhook-url-status"
              className={cn("text-label-sm font-bold", validateWebhookUrl(url.trim()).ok ? 'text-tertiary' : 'text-error')}
            >
              {validateWebhookUrl(url.trim()).ok ? t('settings.notifications.urlStatus.valid') : t('settings.notifications.urlStatus.invalid')}
            </span>
          )}
        </div>
      </div>

      <div>
        <label htmlFor="webhook-secret" className="block font-label-lg text-on-surface mb-sm">
          {t('settings.notifications.secret')}
        </label>
        {/* P1-15: the HMAC secret had state but no control, so it could never
            be set from the UI. Kept password-masked and never re-displayed in
            clear text. */}
        <input
          id="webhook-secret"
          type="password"
          value={secret}
          onChange={(e) => setSecret(e.target.value)}
          placeholder={t('settings.notifications.secretPlaceholder')}
          autoComplete="off"
          className="w-full px-md py-sm rounded-md border border-outline bg-surface text-on-surface focus-visible:border-primary focus-visible:ring-2 focus-visible:ring-primary/30 font-mono"
        />
        <p className="mt-xs text-on-surface-variant font-body-sm">{t('settings.notifications.secretHint')}</p>
      </div>

      {/* P1-7: timeout_ms / include_body always participated in saves but had
          no controls — every save reset them to the defaults. Both are now
          exposed so the round-trip is lossless. */}
      <div className="flex flex-wrap items-end gap-md">
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
            onChange={(e) => handleTimeoutChange(e.target.value)}
            aria-invalid={timeoutInvalid}
            aria-describedby={timeoutInvalid ? 'webhook-timeout-error' : undefined}
            className="w-40 px-md py-sm rounded-md border border-outline bg-surface text-on-surface focus-visible:border-primary focus-visible:ring-2 focus-visible:ring-primary/30 font-mono"
          />
          {timeoutInvalid && (
            <p id="webhook-timeout-error" className="mt-xs font-label-sm text-error">
              {t('settings.notifications.error.timeoutInvalid')}
            </p>
          )}
        </div>
        <div className="flex items-center justify-between gap-md pb-sm">
          <span className="font-label-md text-on-surface">
            {t('settings.notifications.includeBody')}
          </span>
          <Switch
            checked={includeBody}
            onCheckedChange={setIncludeBody}
            aria-label={t('settings.notifications.includeBody')}
            className="shrink-0"
          />
        </div>
      </div>

      {preset === 'custom' && (
        <div>
          <label htmlFor="webhook-custom-body" className="block font-label-lg text-on-surface mb-sm">
            {t('settings.notifications.customBody')}
          </label>
          <textarea
            id="webhook-custom-body"
            value={customBody}
            onChange={(e) => setCustomBody(e.target.value)}
            placeholder={'{"text": "{title}: {body}"}'}
            rows={4}
            className="w-full px-md py-sm rounded-md border border-outline bg-surface text-on-surface focus-visible:border-primary focus-visible:ring-2 focus-visible:ring-primary/30 font-mono font-body-sm"
          />
          <p className="mt-xs text-on-surface-variant font-body-sm">{t('settings.notifications.customBodyHint')}</p>
        </div>
      )}

      <div className="flex flex-wrap gap-sm pt-md">
        {/* P1-15: saving the Custom preset with an empty body would overwrite
            whatever template is currently stored with an empty payload — the
            save is blocked until a body is provided. */}
        <Button
          onClick={handleSave}
          disabled={saving || !url.trim() || !validateWebhookUrl(url.trim()).ok || (preset === 'custom' && customBody.trim() === '')}
        >
          {saving ? t('settings.notifications.saving') : t('settings.notifications.save')}
        </Button>
        {/* R2-P1-7: the test POSTs the SAVED config, so it is only enabled
            when the form matches what is persisted (not dirty) — otherwise
            the verdict would describe the old config, not what the user
            sees. */}
        <Button
          variant="outline"
          onClick={handleTestWebhook}
          disabled={testingWebhook || !url.trim() || webhookDirty}
          aria-describedby={webhookDirty && url.trim() ? 'webhook-test-dirty-hint' : undefined}
        >
          {testingWebhook
            ? t('settings.notifications.sending')
            : t('settings.notifications.webhookTest.button')}
        </Button>
      </div>
      {webhookDirty && url.trim() && (
        <p id="webhook-test-dirty-hint" className="text-on-surface-variant font-body-sm -mt-xs">
          {t('settings.notifications.webhookTest.dirtyHint')}
        </p>
      )}

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
 * Desktop-local: webhooks still deliver while DND suppresses OS popups.
 * P1-7: also hosts the "send test notification" button, moved here from the
 * General page so every notification affordance lives on this page. */
function DndSection({ onSaved }: { onSaved?: () => void } = {}) {
  const t = useT()
  const notify = useNotification()
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  // P1-7: desktop-notification test send (relocated from GeneralSettings).
  const [testingNotification, setTestingNotification] = useState(false)
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
      onSaved?.()
    } catch (e) {
      toastError(t('settings.notifications.dnd.saveFailed'), e)
    } finally {
      setSaving(false)
    }
  }

  const windowDisabled = loading || !master

  // P1-7: fire a native OS notification to verify the renderer is wired up.
  const handleTestNotification = async () => {
    setTestingNotification(true)
    try {
      await notify({
        title: t('settings.notifications.testTitle'),
        body: t('settings.notifications.testBody'),
        level: 'info',
      })
      toast.success(t('settings.notifications.testSent'))
    } catch (e) {
      toastError(t('settings.notifications.testFailed'), e)
    }
    setTestingNotification(false)
  }

  return (
    <section className="rounded-xl border border-outline-variant/30 bg-surface-container-lowest p-lg space-y-md">
      <div className="flex items-center justify-between gap-md">
        <div>
          <div className="font-label-md text-body-sm text-on-surface font-semibold mb-xs">
            {t('settings.notifications.dnd.master')}
          </div>
          <div className="font-label-sm text-label-sm text-on-surface-variant leading-tight">
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
        <div className="font-label-sm text-label-sm uppercase tracking-wide text-on-surface-variant mb-sm">
          {t('settings.notifications.dnd.eventsTitle')}
        </div>
        <div className="space-y-md">
          <div className="flex items-center justify-between gap-md">
            <div>
              <div className="font-label-md text-body-sm text-on-surface font-semibold mb-xs">
                {t('settings.notifications.dnd.completed')}
              </div>
              <div className="font-label-sm text-label-sm text-on-surface-variant leading-tight">
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
              <div className="font-label-md text-body-sm text-on-surface font-semibold mb-xs">
                {t('settings.notifications.dnd.failed')}
              </div>
              <div className="font-label-sm text-label-sm text-on-surface-variant leading-tight">
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
          <div className="font-label-md text-body-sm text-on-surface font-semibold mb-xs">
            {t('settings.notifications.dnd.quietHours')}
          </div>
          <div className="font-label-sm text-label-sm text-on-surface-variant leading-tight">
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
            <span className="font-label-sm text-label-sm text-on-surface-variant">
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
            <span className="font-label-sm text-label-sm text-on-surface-variant">
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
          <p className="sm:col-span-2 font-label-sm text-label-sm text-on-surface-variant">
            {t('settings.notifications.dnd.windowHint')}
          </p>
        </div>
      )}

      <div className="flex flex-wrap items-center gap-sm pt-xs">
        <Button onClick={handleSave} disabled={loading || saving}>
          {saving ? t('settings.notifications.dnd.saving') : t('settings.notifications.dnd.save')}
        </Button>
        {/* P1-7: relocated from the General page — fires a native OS
            notification to verify the desktop channel end-to-end. */}
        <Button
          variant="outline"
          onClick={handleTestNotification}
          disabled={testingNotification}
        >
          {testingNotification
            ? t('settings.notifications.sending')
            : t('settings.notifications.testButton')}
        </Button>
      </div>
    </section>
  )
}

export default function NotificationsSettings() {
  const t = useT()
  // Audit §P2-3 (round 6): a first-time visitor used to see two blank
  // forms with no on-ramp guidance. We now peek at the persisted config
  // and render a setup card while no webhook and no DND events exist.
  const [hasWebhook, setHasWebhook] = useState<boolean | null>(null)
  const [hasDndEvents, setHasDndEvents] = useState<boolean | null>(null)
  // Bump after every save in the child sections so the empty-state check
  // re-runs against the freshly-persisted config.
  const [refreshTick, setRefreshTick] = useState(0)

  useEffect(() => {
    let cancelled = false
    Promise.all([
      api.getWebhookConfig().catch(() => null),
      api.getNotificationPrefs().catch(() => null),
    ]).then(([webhook, prefs]) => {
      if (cancelled) return
      setHasWebhook(Boolean(webhook?.url?.trim()))
      setHasDndEvents(Boolean(prefs?.on_completed || prefs?.on_failed))
    })
    return () => {
      cancelled = true
    }
  }, [refreshTick])

  const showEmptyGuidance =
    hasWebhook === false && hasDndEvents === false

  return (
    <div className="pb-xl">
      <p className="text-on-surface-variant font-body-md mb-md">
        {t('settings.notifications.subtitle')}
      </p>

      {showEmptyGuidance && (
        <div
          role="region"
          aria-label={t('settings.notifications.empty.aria')}
          className="mb-xl p-lg rounded-2xl border border-outline-variant/30 bg-surface-container-low flex flex-col sm:flex-row items-start gap-md"
        >
          <div className="w-12 h-12 rounded-xl bg-primary-container/30 flex items-center justify-center shrink-0">
            <span className="material-symbols-outlined icon-xl text-primary" aria-hidden="true">
              notifications_active
            </span>
          </div>
          <div className="flex-1 min-w-0">
            <h3 className="font-headline-sm text-on-surface mb-xs">
              {t('settings.notifications.empty.title')}
            </h3>
            <p className="text-on-surface-variant font-body-sm mb-md max-w-prose">
              {t('settings.notifications.empty.desc')}
            </p>
            <ul className="text-on-surface-variant font-body-sm space-y-xs list-disc pl-lg">
              <li>{t('settings.notifications.empty.bullet.webhook')}</li>
              <li>{t('settings.notifications.empty.bullet.dnd')}</li>
            </ul>
          </div>
        </div>
      )}

      <section className="mt-xl">
        <div className="mb-lg">
          <h3 className="font-headline-md text-on-surface mb-xs">{t('settings.notifications.dnd.sectionTitle')}</h3>
          <p className="text-on-surface-variant font-body-sm">{t('settings.notifications.dnd.sectionDesc')}</p>
        </div>
        <DndSection onSaved={() => setRefreshTick((n) => n + 1)} />
      </section>

      <section className="mt-xl">
        <div className="mb-lg">
          <h3 className="font-headline-md text-on-surface mb-xs">{t('settings.notifications.webhook.title')}</h3>
          <p className="text-on-surface-variant font-body-sm">{t('settings.notifications.webhook.subtitle')}</p>
        </div>
        <WebhookSection onSaved={() => setRefreshTick((n) => n + 1)} />
      </section>
    </div>
  )
}
