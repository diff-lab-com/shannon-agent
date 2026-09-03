import { useState } from 'react'
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import { CodeBlock } from '@/components/code/CodeBlock'
import { cn } from '@/lib/utils'
import type { SlashResult } from '@/lib/slash/commands'

interface SlashResultCardProps {
  result: SlashResult
  onDismiss: () => void
}

/* Ephemeral output surface for composer slash commands (/context /cost
 * /diff). Deliberately NOT a chat message: results are diagnostics about
 * the session, not turns in it, so they must not pollute the L0 log the
 * way a synthetic message would. Pinned above the composer, dismissible. */
export default function SlashResultCard({ result, onDismiss }: SlashResultCardProps) {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, string | number>) =>
    intl.formatMessage({ id }, values)
  const [showPatch, setShowPatch] = useState(false)

  const fmt = new Intl.NumberFormat(intl.locale)
  const usd = (v: number) =>
    new Intl.NumberFormat(intl.locale, { style: 'currency', currency: 'USD' }).format(v)

  let body: React.ReactNode = null
  if (result.kind === 'context') {
    const { estimated_tokens: tokens, context_window: window } = result.stats
    const pct = window ? Math.min(100, Math.round((tokens / window) * 100)) : null
    body = (
      <>
        <div className="flex items-baseline gap-xs">
          <span className="font-label-lg font-bold text-on-surface tabular-nums">{fmt.format(tokens)}</span>
          <span className="font-label-sm text-on-surface-variant">
            {window
              ? t('slash.card.context.ofWindow', { window: fmt.format(window) })
              : t('slash.card.context.windowUnknown')}
          </span>
        </div>
        {pct !== null && (
          <div
            role="progressbar"
            aria-valuenow={pct}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-label={t('slash.card.context.title')}
            className="mt-xs h-1.5 rounded-full bg-surface-container-highest overflow-hidden"
          >
            <div
              className={cn('h-full rounded-full', pct > 90 ? 'bg-error' : pct > 70 ? 'bg-tertiary' : 'bg-primary')}
              style={{ width: `${pct}%` }}
            />
          </div>
        )}
        <p className="mt-xs font-label-xs text-on-surface-variant">{t('slash.card.context.hint')}</p>
      </>
    )
  } else if (result.kind === 'cost') {
    const u = result.usage
    body = (
      <div className="flex flex-col gap-xxs">
        <div className="flex items-baseline gap-xs">
          <span className="font-label-lg font-bold text-on-surface tabular-nums">{usd(u.cost_usd)}</span>
          <span className="font-label-sm text-on-surface-variant">
            {t('slash.card.cost.overEvents', { count: fmt.format(u.events) })}
          </span>
        </div>
        <dl className="grid grid-cols-[auto_1fr] gap-x-md gap-y-xxs font-label-sm">
          <dt className="text-on-surface-variant">{t('slash.card.cost.input')}</dt>
          <dd className="text-on-surface tabular-nums">{fmt.format(u.input_tokens)}</dd>
          <dt className="text-on-surface-variant">{t('slash.card.cost.output')}</dt>
          <dd className="text-on-surface tabular-nums">{fmt.format(u.output_tokens)}</dd>
          <dt className="text-on-surface-variant">{t('slash.card.cost.cacheRead')}</dt>
          <dd className="text-on-surface tabular-nums">{fmt.format(u.cache_read_tokens)}</dd>
        </dl>
        {u.events === 0 && (
          <p className="font-label-xs text-on-surface-variant">{t('slash.card.cost.legacyHint')}</p>
        )}
      </div>
    )
  } else if (result.kind === 'diff') {
    if (!result.diff.is_repo) {
      body = <p className="font-label-sm text-on-surface-variant">{t('slash.card.diff.notRepo')}</p>
    } else if (result.diff.files.length === 0) {
      body = <p className="font-label-sm text-on-surface-variant">{t('slash.card.diff.noChanges')}</p>
    } else {
      body = (
        <div className="flex flex-col gap-xs">
          <p className="font-label-sm text-on-surface">
            {t('slash.card.diff.fileCount', { count: result.diff.files.length })}
            {result.diff.truncated && (
              <span className="text-on-surface-variant"> · {t('slash.card.diff.truncated')}</span>
            )}
          </p>
          <ul className="flex flex-col gap-xxs">
            {result.diff.files.map(f => (
              <li key={f.path} className="flex items-center gap-sm font-mono text-label-sm">
                <span className="text-on-surface truncate flex-1">{f.path}</span>
                <span className="text-green-600 tabular-nums">+{f.insertions}</span>
                <span className="text-error tabular-nums">−{f.deletions}</span>
              </li>
            ))}
          </ul>
          {result.diff.patch && (
            <div>
              <Button
                variant="ghost"
                size="sm"
                className="h-auto px-0 text-label-sm text-primary"
                aria-expanded={showPatch}
                onClick={() => setShowPatch(v => !v)}
              >
                {showPatch ? t('slash.card.diff.hidePatch') : t('slash.card.diff.showPatch')}
              </Button>
              {showPatch && <CodeBlock code={result.diff.patch} language="diff" className="my-xs" />}
            </div>
          )}
        </div>
      )
    }
  } else {
    body = <p className="font-label-sm text-on-surface">{t(result.messageKey)}</p>
  }

  const titleKey =
    result.kind === 'context' ? 'slash.card.context.title'
    : result.kind === 'cost' ? 'slash.card.cost.title'
    : result.kind === 'diff' ? 'slash.card.diff.title'
    : 'slash.card.error.title'

  return (
    <div
      role="status"
      className="mx-md mt-md p-md rounded-xl border border-outline-variant/30 bg-surface-container/50"
    >
      <div className="flex items-start justify-between gap-sm">
        <h4 className="font-label-md font-bold text-on-surface-variant flex items-center gap-xs">
          <span className="material-symbols-outlined text-[16px]">
            {result.kind === 'context' ? 'data_usage'
              : result.kind === 'cost' ? 'payments'
              : result.kind === 'diff' ? 'difference'
              : 'error'}
          </span>
          {t(titleKey)}
        </h4>
        <Button
          variant="ghost"
          size="icon-xs"
          aria-label={t('slash.card.dismiss')}
          onClick={onDismiss}
          className="text-on-surface-variant hover:text-on-surface shrink-0"
        >
          <span className="material-symbols-outlined icon-sm">close</span>
        </Button>
      </div>
      <div className="mt-xs">{body}</div>
    </div>
  )
}
