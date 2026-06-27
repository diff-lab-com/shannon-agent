import { useEffect, useState } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import * as api from '@/lib/tauri-api'
import { toastError } from '@/lib/errorToast'

interface ResultRowProps {
  provider: string
  ok: boolean
  error?: string | null
}

function ResultRow({ provider, ok, error }: ResultRowProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  return (
    <div
      className={`flex items-center gap-sm px-md py-sm rounded-md text-sm ${
        ok ? 'bg-tertiary/10 text-tertiary' : 'bg-error/10 text-error'
      }`}
      role="status"
    >
      <span
        className="material-symbols-outlined text-[18px]"
        aria-hidden="true"
      >
        {ok ? 'check_circle' : 'cancel'}
      </span>
      <span className="font-label-md">{provider}</span>
      {!ok && error && (
        <span className="ml-auto truncate opacity-80" title={error}>
          {error}
        </span>
      )}
      <span className="sr-only">
        {ok
          ? t('settings.notifications.outbound.result.success')
          : t('settings.notifications.outbound.result.failed')}
      </span>
    </div>
  )
}

export default function OutboundSection() {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [sending, setSending] = useState(false)
  const [clearing, setClearing] = useState(false)
  const [slackToken, setSlackToken] = useState('')
  const [slackChannel, setSlackChannel] = useState('')
  const [telegramToken, setTelegramToken] = useState('')
  const [telegramChat, setTelegramChat] = useState('')
  const [testMessage, setTestMessage] = useState('')
  const [results, setResults] = useState<api.ChannelResult[] | null>(null)

  useEffect(() => {
    let cancelled = false
    api
      .getOutboundConfig()
      .then((cfg) => {
        if (cancelled) return
        setSlackToken(cfg.slack?.bot_token ?? '')
        setSlackChannel(cfg.slack?.channel ?? '')
        setTelegramToken(cfg.telegram?.bot_token ?? '')
        setTelegramChat(cfg.telegram?.chat_id ?? '')
      })
      .catch((e) => console.warn('getOutboundConfig error:', e))
      .finally(() => {
        if (!cancelled) setLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [])

  const handleSave = async () => {
    const hasSlack = slackToken.trim() && slackChannel.trim()
    const hasTelegram = telegramToken.trim() && telegramChat.trim()
    if (!hasSlack && !hasTelegram) {
      toast.error(t('settings.notifications.outbound.error.empty'))
      return
    }
    setSaving(true)
    try {
      const dto: api.OutboundConfigDto = {
        slack: hasSlack
          ? { bot_token: slackToken.trim(), channel: slackChannel.trim() }
          : null,
        telegram: hasTelegram
          ? { bot_token: telegramToken.trim(), chat_id: telegramChat.trim() }
          : null,
      }
      await api.saveOutboundConfig(dto)
      toast.success(t('settings.notifications.outbound.saved'))
    } catch (e) {
      toastError(t('settings.notifications.outbound.error.saveFailed'), e)
    }
    setSaving(false)
  }

  const handleClear = async () => {
    setClearing(true)
    try {
      await api.clearOutboundConfig()
      setSlackToken('')
      setSlackChannel('')
      setTelegramToken('')
      setTelegramChat('')
      setResults(null)
      toast.success(t('settings.notifications.outbound.cleared'))
    } catch (e) {
      toastError(t('settings.notifications.outbound.error.clearFailed'), e)
    }
    setClearing(false)
  }

  const handleSendTest = async () => {
    setSending(true)
    setResults(null)
    try {
      const outcome = await api.sendOutboundTest(testMessage.trim())
      setResults(outcome.results)
      if (outcome.results.length === 0) {
        toast.error(t('settings.notifications.outbound.error.noChannels'))
      } else if (outcome.results.every((r) => r.ok)) {
        toast.success(t('settings.notifications.outbound.test.allOk'))
      } else if (outcome.results.some((r) => r.ok)) {
        toast.warning(t('settings.notifications.outbound.test.partial'))
      } else {
        toast.error(t('settings.notifications.outbound.test.allFailed'))
      }
    } catch (e) {
      toastError(t('settings.notifications.outbound.test.allFailed'), e)
    }
    setSending(false)
  }

  if (loading) {
    return (
      <div
        className="flex items-center justify-center py-12"
        role="status"
        aria-live="polite"
      >
        <span
          className="material-symbols-outlined icon-xl text-primary animate-spin"
          aria-hidden="true"
        >
          progress_activity
        </span>
        <span className="sr-only">{t('settings.notifications.loading')}</span>
      </div>
    )
  }

  return (
    <div className="bg-surface-container-lowest p-lg rounded-xl shadow-sm border border-outline-variant/30 space-y-md">
      <div>
        <h4 className="font-headline-md text-on-surface">
          {t('settings.notifications.outbound.title')}
        </h4>
        <p className="text-on-surface-variant font-body-sm">
          {t('settings.notifications.outbound.subtitle')}
        </p>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-md">
        <div className="space-y-sm">
          <div className="flex items-center gap-xs">
            <span className="material-symbols-outlined text-primary" aria-hidden="true">
              tag
            </span>
            <h4 className="font-label-md text-on-surface">
              {t('settings.notifications.outbound.slack.header')}
            </h4>
          </div>
          <input
            type="password"
            value={slackToken}
            onChange={(e) => setSlackToken(e.target.value)}
            placeholder="xoxb-••••••"
            aria-label={t('settings.notifications.outbound.slack.token')}
            className="w-full px-md py-sm rounded-md border border-outline bg-surface text-on-surface focus-visible:border-primary focus-visible:ring-2 focus-visible:ring-primary/30"
          />
          <input
            type="text"
            value={slackChannel}
            onChange={(e) => setSlackChannel(e.target.value)}
            placeholder="#general or C012345"
            aria-label={t('settings.notifications.outbound.slack.channel')}
            className="w-full px-md py-sm rounded-md border border-outline bg-surface text-on-surface focus-visible:border-primary focus-visible:ring-2 focus-visible:ring-primary/30"
          />
        </div>

        <div className="space-y-sm">
          <div className="flex items-center gap-xs">
            <span className="material-symbols-outlined text-primary" aria-hidden="true">
              send
            </span>
            <h4 className="font-label-md text-on-surface">
              {t('settings.notifications.outbound.telegram.header')}
            </h4>
          </div>
          <input
            type="password"
            value={telegramToken}
            onChange={(e) => setTelegramToken(e.target.value)}
            placeholder="1234567890:ABC..."
            aria-label={t('settings.notifications.outbound.telegram.token')}
            className="w-full px-md py-sm rounded-md border border-outline bg-surface text-on-surface focus-visible:border-primary focus-visible:ring-2 focus-visible:ring-primary/30"
          />
          <input
            type="text"
            value={telegramChat}
            onChange={(e) => setTelegramChat(e.target.value)}
            placeholder="@channel or -1001234567890"
            aria-label={t('settings.notifications.outbound.telegram.chatId')}
            className="w-full px-md py-sm rounded-md border border-outline bg-surface text-on-surface focus-visible:border-primary focus-visible:ring-2 focus-visible:ring-primary/30"
          />
        </div>
      </div>

      <div className="flex flex-wrap gap-sm pt-md">
        <Button onClick={handleSave} disabled={saving}>
          {saving
            ? t('settings.notifications.saving')
            : t('settings.notifications.save')}
        </Button>
        <Button variant="outline" onClick={handleClear} disabled={clearing}>
          {clearing
            ? t('settings.notifications.clearing')
            : t('settings.notifications.clear')}
        </Button>
      </div>

      <div className="pt-md border-t border-outline-variant/30 space-y-sm">
        <h4 className="font-label-md text-on-surface">
          {t('settings.notifications.outbound.test.title')}
        </h4>
        <div className="flex gap-sm">
          <input
            type="text"
            value={testMessage}
            onChange={(e) => setTestMessage(e.target.value)}
            placeholder={t('settings.notifications.outbound.test.placeholder')}
            aria-label={t('settings.notifications.outbound.test.messageLabel')}
            className="flex-1 px-md py-sm rounded-md border border-outline bg-surface text-on-surface focus-visible:border-primary focus-visible:ring-2 focus-visible:ring-primary/30"
          />
          <Button onClick={handleSendTest} disabled={sending}>
            {sending ? (
              <span
                className="material-symbols-outlined text-[18px] animate-spin mr-xs"
                aria-hidden="true"
              >
                progress_activity
              </span>
            ) : (
              <span
                className="material-symbols-outlined text-[18px] mr-xs"
                aria-hidden="true"
              >
                send
              </span>
            )}
            {t('settings.notifications.outbound.test.send')}
          </Button>
        </div>
        {results && results.length > 0 && (
          <div className="space-y-xs pt-sm">
            {results.map((r) => (
              <ResultRow
                key={r.provider}
                provider={r.provider}
                ok={r.ok}
                error={r.error}
              />
            ))}
          </div>
        )}
      </div>
    </div>
  )
}
