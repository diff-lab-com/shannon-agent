// PM-12 display half (§8): surface the persisted 👍/👎 signal as a compact
// per-session aggregate in Settings → General, closing the feedback loop —
// until now the only way to see reactions was reading the JSON files.
import { useEffect, useState } from 'react'
import { useIntl } from 'react-intl'
import * as api from '@/lib/tauri-api'
import type { FeedbackSessionSummary } from '@/lib/tauri-api'

export function FeedbackSummaryCard() {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const [rows, setRows] = useState<FeedbackSessionSummary[] | null>(null)

  useEffect(() => {
    let cancelled = false
    api
      .listFeedbackSessions()
      .then(r => {
        if (!cancelled) setRows(r)
      })
      .catch(() => {
        if (!cancelled) setRows([])
      })
    return () => {
      cancelled = true
    }
  }, [])

  return (
    <section className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-xl shadow-sm">
      <div className="flex items-center gap-md mb-xs">
        <span className="material-symbols-outlined text-primary" style={{ fontVariationSettings: "'FILL' 1" }}>thumb_up</span>
        <h3 className="font-headline-md text-headline-md">{t('settings.feedback.title')}</h3>
      </div>
      <p className="font-body-sm text-on-surface-variant mb-md">{t('settings.feedback.help')}</p>
      {rows === null ? (
        <p className="font-label-md text-on-surface-variant">{t('settings.feedback.loading')}</p>
      ) : rows.length === 0 ? (
        <p className="font-label-md text-on-surface-variant italic">{t('settings.feedback.empty')}</p>
      ) : (
        <ul className="flex flex-col gap-xs" aria-label={t('settings.feedback.listAria')}>
          {rows.map(r => (
            <li key={r.session_id} className="flex items-center justify-between gap-md rounded-lg bg-surface-container-low/60 px-md py-xs">
              <span className="font-mono text-label-sm text-on-surface truncate" title={r.session_id}>
                {r.session_id.slice(0, 8)}
              </span>
              <span className="flex items-center gap-md text-label-sm tabular-nums">
                <span className="flex items-center gap-1 text-primary" aria-label={t('settings.feedback.upAria')}>
                  <span className="material-symbols-outlined text-[16px]" aria-hidden="true">thumb_up</span>
                  {r.up}
                </span>
                <span className="flex items-center gap-1 text-error" aria-label={t('settings.feedback.downAria')}>
                  <span className="material-symbols-outlined text-[16px]" aria-hidden="true">thumb_down</span>
                  {r.down}
                </span>
              </span>
            </li>
          ))}
        </ul>
      )}
    </section>
  )
}
