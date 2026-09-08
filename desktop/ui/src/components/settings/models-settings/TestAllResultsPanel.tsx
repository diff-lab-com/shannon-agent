import type { useIntl } from 'react-intl'
import type * as api from '@/lib/tauri-api'
import { cn } from '@/lib/utils'

/**
 * Renders the results of the "Test all providers" fan-out probe as a compact
 * list of provider label + status pill + latency. Status pill reuses the
 * same `settings.models.testResult.*` keys the single-provider toast uses so
 * the wording stays identical between the two surfaces.
 */
export function TestAllResultsPanel({
  rows,
  intl,
  t,
}: {
  rows: api.ProviderTestRow[]
  intl: ReturnType<typeof useIntl>
  t: (id: string) => string
}) {
  const okCount = rows.filter(r => r.result.kind === 'success').length
  const summary = intl.formatMessage(
    { id: 'settings.models.providers.testAllSummary' },
    { ok: okCount, total: rows.length },
  )
  return (
    <div
      data-testid="test-all-results"
      className="mt-md rounded-lg border border-outline-variant/30 bg-surface-container-low/30 p-md space-y-sm"
    >
      <p className="font-label-sm text-on-surface-variant">{summary}</p>
      <div className="grid grid-cols-1 gap-xs">
        {rows.map(r => {
          const result = r.result
          const pillClass =
            result.kind === 'success'
              ? 'bg-primary-container text-on-primary-container'
              : result.kind === 'rate_limited'
                ? 'bg-tertiary-container text-on-tertiary-container'
                : 'bg-error-container text-on-error-container'
          const pillLabel = (() => {
            switch (result.kind) {
              case 'success':
                return t('settings.models.testResult.success')
              case 'invalid_key':
                return t('settings.models.testResult.invalidKey')
              case 'rate_limited':
                return t('settings.models.testResult.rateLimited')
              case 'network_unreachable':
                return intl.formatMessage({ id: 'settings.models.testResult.networkUnreachable' }, { provider: r.provider_kind })
              case 'provider_error':
                return intl.formatMessage({ id: 'settings.models.testResult.providerError' }, { provider: r.provider_kind, status: result.status })
              case 'unknown':
                return intl.formatMessage({ id: 'settings.models.testResult.unknown' }, { message: result.message })
            }
          })()
          return (
            <div
              key={r.id}
              className="flex items-center justify-between gap-md px-sm py-xs rounded-md bg-surface-container-lowest"
            >
              <div className="flex items-center gap-sm min-w-0">
                <span className="font-label-md text-on-surface truncate">{r.label}</span>
                <span className="font-label-xs text-[11px] text-on-surface-variant">{r.provider_kind}</span>
              </div>
              <div className="flex items-center gap-sm shrink-0">
                {r.latency_ms !== null ? (
                  <span className="font-label-xs text-[11px] text-on-surface-variant">
                    {intl.formatMessage({ id: 'settings.models.providers.latency' }, { ms: r.latency_ms })}
                  </span>
                ) : null}
                <span
                  data-testid={`test-all-result-${r.id}`}
                  className={cn("px-sm py-[2px] rounded-full text-[10px] font-bold uppercase tracking-wider", pillClass)}
                  title={pillLabel}
                >
                  {pillLabel}
                </span>
              </div>
            </div>
          )
        })}
      </div>
    </div>
  )
}