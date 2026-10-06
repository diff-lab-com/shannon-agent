// S3-4 (推荐降级链) — the Test-all results panel carries the "Suggest
// fallback chain" affordance on exactly the failure classes failover cures:
// rate-limited (429) and quota-exhausted (402). Invalid keys, unreachable
// networks and provider errors are NOT load problems — no affordance there,
// and success rows never offer one.
import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { TestAllResultsPanel } from '@/components/settings/models-settings/TestAllResultsPanel'
import type * as api from '@/lib/tauri-api'
import type { ProviderTestRow } from '@/lib/tauri-api'

const t = (id: string) => id
/** Minimal intl stub: the panel only reads `formatMessage({ id }, values)`. */
const intl = {
  formatMessage: ({ id }: { id: string }, values?: Record<string, unknown>) =>
    id + (values ? ` ${JSON.stringify(values)}` : ''),
} as unknown as Parameters<typeof TestAllResultsPanel>[0]['intl']

function row(overrides: Partial<ProviderTestRow>): ProviderTestRow {
  return {
    id: 'prov-a',
    label: 'Provider A',
    provider_kind: 'anthropic',
    result: { kind: 'success' },
    latency_ms: null,
    ...overrides,
  }
}

function renderPanel(rows: ProviderTestRow[], onSuggestFallback?: (id: string) => void) {
  return render(
    <TestAllResultsPanel
      rows={rows as api.ProviderTestRow[]}
      intl={intl}
      t={t}
      onSuggestFallback={onSuggestFallback}
    />,
  )
}

describe('TestAllResultsPanel — fallback suggestion affordance (S3-4)', () => {
  it('a rate-limited row offers the fallback suggestion and reports its provider id', () => {
    const onSuggestFallback = vi.fn()
    renderPanel(
      [row({ id: 'prov-rl', result: { kind: 'rate_limited' } })],
      onSuggestFallback,
    )
    const btn = screen.getByTestId('test-all-fallback-prov-rl')
    fireEvent.click(btn)
    expect(onSuggestFallback).toHaveBeenCalledWith('prov-rl')
  })

  it('a quota-exhausted (402) row offers the fallback suggestion too', () => {
    const onSuggestFallback = vi.fn()
    renderPanel(
      [row({ id: 'prov-q', result: { kind: 'quota_exhausted' } })],
      onSuggestFallback,
    )
    fireEvent.click(screen.getByTestId('test-all-fallback-prov-q'))
    expect(onSuggestFallback).toHaveBeenCalledWith('prov-q')
  })

  it('success / invalid-key / network rows never offer one', () => {
    renderPanel(
      [
        row({ id: 'ok', result: { kind: 'success' } }),
        row({ id: 'badkey', result: { kind: 'invalid_key' } }),
        row({ id: 'net', result: { kind: 'network_unreachable' } }),
        row({ id: 'perr', result: { kind: 'provider_error', status: 503 } }),
      ],
      vi.fn(),
    )
    expect(screen.queryByTestId('test-all-fallback-ok')).not.toBeInTheDocument()
    expect(screen.queryByTestId('test-all-fallback-badkey')).not.toBeInTheDocument()
    expect(screen.queryByTestId('test-all-fallback-net')).not.toBeInTheDocument()
    expect(screen.queryByTestId('test-all-fallback-perr')).not.toBeInTheDocument()
  })

  it('renders without the callback (standalone mount) — failed rows just show the pill', () => {
    renderPanel([row({ id: 'prov-rl', result: { kind: 'rate_limited' } })])
    expect(screen.getByTestId('test-all-result-prov-rl')).toBeInTheDocument()
    expect(screen.queryByTestId('test-all-fallback-prov-rl')).not.toBeInTheDocument()
  })
})
