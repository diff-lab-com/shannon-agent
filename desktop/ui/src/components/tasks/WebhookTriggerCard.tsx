import { useState } from 'react'
import { Button } from '@/components/ui/button'
import { useT } from '@/i18n'
import type { ScheduledRoutine } from '@/types'

// Loopback inbound-trigger endpoint (desktop/src/loopback_api.rs). One URL
// per routine; auth is the same HMAC secret as the notification webhooks.
const TRIGGER_BASE = 'http://127.0.0.1:33420/api/routines'

/**
 * Inbound webhook triggers card (audit G2 last-mile): lists every
 * webhook-type routine with its copyable POST endpoint, so automation
 * authors (GitHub pushes, IM bots, CI) can wire external events to it
 * without digging through the backend source.
 */
export default function WebhookTriggerCard({ routines }: { routines: ScheduledRoutine[] }) {
  const t = useT()
  const [copiedId, setCopiedId] = useState<string | null>(null)
  const webhookRoutines = routines.filter(r => r.trigger_type === 'webhook' && r.enabled)

  if (webhookRoutines.length === 0) return null

  const copy = async (routine: ScheduledRoutine) => {
    const url = `${TRIGGER_BASE}/${routine.id}/trigger`
    try {
      await navigator.clipboard.writeText(url)
      setCopiedId(routine.id)
      setTimeout(() => setCopiedId(null), 1500)
    } catch {
      // Clipboard may be denied in some webviews — the URL is selectable text.
    }
  }

  return (
    <section
      aria-labelledby="webhook-triggers-heading"
      className="bg-surface-container-lowest border border-outline-variant/30 rounded-xl p-lg"
      data-testid="webhook-trigger-card"
    >
      <h2 id="webhook-triggers-heading" className="font-label-lg font-bold text-on-surface mb-sm flex items-center gap-xs">
        <span className="material-symbols-outlined text-[18px] text-primary" aria-hidden="true">webhook</span>
        {t('tasks.webhookTrigger.heading')}
      </h2>
      <p className="font-body-sm text-on-surface-variant mb-md">{t('tasks.webhookTrigger.hint')}</p>
      <ul className="space-y-sm">
        {webhookRoutines.map(r => (
          <li key={r.id} className="flex items-center gap-sm flex-wrap p-sm rounded-lg bg-surface-container-low border border-outline-variant/20">
            <span className="font-label-md text-on-surface font-medium truncate max-w-[240px]">{r.name}</span>
            <code className="flex-1 min-w-[200px] truncate font-mono text-[11px] text-on-surface-variant px-sm py-1 rounded bg-surface-container-lowest border border-outline-variant/20">
              POST {TRIGGER_BASE}/{r.id}/trigger
            </code>
            <Button
              variant="outline"
              size="sm"
              onClick={() => void copy(r)}
              className="cursor-pointer shrink-0"
              aria-label={t('tasks.webhookTrigger.copy.aria', { name: r.name })}
            >
              <span className="material-symbols-outlined icon-sm" aria-hidden="true">
                {copiedId === r.id ? 'check' : 'content_copy'}
              </span>
              {copiedId === r.id ? t('tasks.webhookTrigger.copied') : t('tasks.webhookTrigger.copy')}
            </Button>
          </li>
        ))}
      </ul>
    </section>
  )
}
