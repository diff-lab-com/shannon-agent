import type { useIntl } from 'react-intl'
import type * as api from '@/lib/tauri-api'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

/**
 * Renders the results of the "Test all providers" fan-out probe as a compact
 * list of provider label + status pill + latency. Status pill reuses the
 * same `settings.models.testResult.*` keys the single-provider toast uses so
 * the wording stays identical between the two surfaces.
 *
 * S3-4: a rate-limited / quota-exhausted row is exactly where a fallback
 * chain pays off, so those rows carry a one-click "Suggest fallback chain"
 * affordance (same panel the provider card opens). Optional callback — the
 * panel renders fine without it (standalone mounts in tests/storybook).
 */
export function TestAllResultsPanel({
  rows,
  intl,
  t,
  onSuggestFallback,
}: {
  rows: api.ProviderTestRow[]
  intl: ReturnType<typeof useIntl>
  t: (id: string) => string
  /** S3-4: open the recommended-fallback-chain panel for a provider id. */
  onSuggestFallback?: (providerId: string) => void
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
          // S3-4: rate-limited and quota-exhausted providers are the exact
          // failure class failover cures — offer the chain suggestion there
          // (and only there; an invalid key or unreachable network is not a
          // load problem a fallback chain solves).
          const fallbackWorthy = result.kind === 'rate_limited' || result.kind === 'quota_exhausted'
          const pillLabel = (() => {
            switch (result.kind) {
              case 'success':
                return t('settings.models.testResult.success')
              case 'invalid_key':
                return t('settings.models.testResult.invalidKey')
              case 'rate_limited':
                return t('settings.models.testResult.rateLimited')
              // R2-P1-10: HTTP 402 — quota/billing exhausted, not a bad key.
              case 'quota_exhausted':
                return t('settings.models.testResult.quotaExhausted')
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
                <span className="font-label-xs text-label-xs text-on-surface-variant">{r.provider_kind}</span>
              </div>
              <div className="flex items-center gap-sm shrink-0">
                {r.latency_ms !== null ? (
                  <span className="font-label-xs text-label-xs text-on-surface-variant">
                    {intl.formatMessage({ id: 'settings.models.providers.latency' }, { ms: r.latency_ms })}
                  </span>
                ) : null}
                {fallbackWorthy && onSuggestFallback ? (
                  <Button
                    variant="ghost"
                    className="px-sm py-xs h-auto text-label-xs text-primary hover:bg-primary/10 cursor-pointer flex items-center gap-[2px]"
                    onClick={() => onSuggestFallback(r.id)}
                    aria-label={t('settings.models.providers.recommendFallback')}
                    title={t('settings.models.providers.recommendFallback')}
                    data-testid={`test-all-fallback-${r.id}`}
                  >
                    <span className="material-symbols-outlined icon-sm" aria-hidden="true">alt_route</span>
                    {t('settings.models.providers.recommendFallback')}
                  </Button>
                ) : null}
                <span
                  data-testid={`test-all-result-${r.id}`}
                  className={cn("px-sm py-[2px] rounded-full text-label-2xs font-bold uppercase tracking-wider", pillClass)}
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