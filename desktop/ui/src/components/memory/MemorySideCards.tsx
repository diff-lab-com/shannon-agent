// Memory page right-rail cards (G4a 页面骨架补齐, design 08-memory: 分类分布
// donut + 梦整理 summary).
//
// CategoryDonutCard — CSS conic-gradient ring (design 08:289-331): slices in
// the semantic token hues matching the CATEGORY_COLOR chip pairs, legend rows
// with per-category counts. Center number = the largest category's share, the
// design's single-figure read.
//
// DreamSummaryCard — compact 「梦整理」 summary (design 08:543-709): title +
// last-pass time + up to two proposal summary lines + a 查看变更 link that
// scrolls to the full DreamPanel review surface (id="dream-panel"). Data
// rides the existing proposals API (list_dream_proposals) + persisted state
// (read_dream_state); no data → the honest empty line.

import { useEffect, useState } from 'react'
import { useIntl } from 'react-intl'
import {
  listDreamProposals,
  readDreamState,
  type DreamProposal,
  type DreamState,
  type MemoryStats,
} from '@/lib/tauri-api'
import { cn } from '@/lib/utils'

/** Slice hue per category — CSS custom properties, so every theme tints the
 *  ring with the same semantic pairs the chips use (G1-B four-color rule).
 *  'error' joins the design's four when the store actually carries errors. */
const CATEGORY_SLICE_COLOR: Record<string, string> = {
  preference: 'var(--color-primary)',
  decision: 'var(--color-secondary)',
  pattern: 'var(--color-outline)',
  context: 'var(--color-success)',
  error: 'var(--color-error)',
}

/** Design 08 legend order: preference / decision / pattern / context, with
 *  the error slice appended only when the store carries one. */
const DONUT_ORDER = ['preference', 'decision', 'pattern', 'context', 'error'] as const

export function CategoryDonutCard({ stats }: { stats: MemoryStats | null }) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  const rows = DONUT_ORDER
    .map((c) => ({ category: c, count: stats?.by_category[c] ?? 0 }))
    .filter((r) => r.count > 0)
  const total = rows.reduce((sum, r) => sum + r.count, 0)

  // conic-gradient stops: each slice spans count/total of the turn.
  let cursor = 0
  const stops = rows.map((r) => {
    const from = (cursor / total) * 360
    cursor += r.count
    const to = (cursor / total) * 360
    return `${CATEGORY_SLICE_COLOR[r.category]} ${from}deg ${to}deg`
  })

  // Center figure: the largest category's share (the design's single number).
  const largest = rows.length > 0
    ? rows.reduce((acc, r) => (r.count > acc.count ? r : acc))
    : null
  const largestPct = largest && total > 0 ? Math.round((largest.count / total) * 100) : 0

  return (
    <section
      aria-label={t('memory.categories.title')}
      data-testid="memory-categories-card"
      className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-md shadow-e1"
    >
      <h2 className="text-label-lg font-semibold text-on-surface mb-md">{t('memory.categories.title')}</h2>
      {total === 0 ? (
        <p className="text-label-sm text-on-surface-variant py-sm">
          {intl.formatMessage({ id: 'memory.listCount' }, { count: 0 })}
        </p>
      ) : (
        <div className="flex items-center gap-md">
          {/* CSS conic-gradient ring — theme-token hues, the inner hole cut by
              a card-colored core (no svg dependency). */}
          <div
            role="img"
            aria-label={t('memory.categories.title')}
            className="relative w-[88px] h-[88px] rounded-full shrink-0"
            style={{ background: `conic-gradient(${stops.join(', ')})` }}
          >
            <div className="absolute inset-[14px] rounded-full bg-surface-container-lowest flex items-center justify-center">
              <span className="text-label-md font-bold text-on-surface tabular-nums">{largestPct}%</span>
            </div>
          </div>
          <ul className="flex-1 min-w-0 space-y-xs">
            {rows.map((r) => (
              <li key={r.category} className="flex items-center gap-xs text-label-sm">
                <span
                  aria-hidden="true"
                  className="w-2 h-2 rounded-full shrink-0"
                  style={{ background: CATEGORY_SLICE_COLOR[r.category] }}
                />
                <span className="flex-1 min-w-0 truncate text-on-surface-variant">
                  {t(`memory.category.${r.category}`)}
                </span>
                <span className="text-on-surface font-bold tabular-nums">{r.count}</span>
              </li>
            ))}
          </ul>
        </div>
      )}
    </section>
  )
}

export function DreamSummaryCard() {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, string | number>) =>
    intl.formatMessage({ id }, values)

  const [proposals, setProposals] = useState<DreamProposal[] | null>(null)
  const [state, setState] = useState<DreamState | null>(null)

  // Best-effort reads — the same commands DreamPanel uses, fetched ONCE on
  // mount (this card is a read-only summary; the live review flow, including
  // refresh-after-apply, stays owned by DreamPanel below the list).
  useEffect(() => {
    let cancelled = false
    listDreamProposals()
      .then((rows) => { if (!cancelled) setProposals(rows ?? []) })
      .catch(() => { if (!cancelled) setProposals([]) })
    readDreamState()
      .then((s) => { if (!cancelled) setState(s) })
      .catch(() => { /* no persisted pass — the time line stays off */ })
    return () => { cancelled = true }
  }, [])

  // Aggregate action totals across pending proposals (the design's summary
  // is a count digest, not the raw rationales — those stay in DreamPanel).
  const totals = { merge: 0, remove: 0, add: 0 }
  for (const p of proposals ?? []) {
    for (const a of p.actions) {
      if (a.kind === 'merge' || a.kind === 'remove' || a.kind === 'add') totals[a.kind]++
    }
  }
  const hasTotals = totals.merge + totals.remove + totals.add > 0
  // Newest pending proposal's project + its date — one composed string so it
  // can never collide with DreamPanel's full proposal rows.
  const newest = (proposals ?? [])[0]
  const newestLine = newest
    ? `${newest.project} · ${intl.formatDate(newest.created_at, { month: 'short', day: 'numeric' })}`
    : null

  const lastAt = state?.last_dream_at ? new Date(state.last_dream_at) : null
  const time = lastAt && !Number.isNaN(lastAt.getTime())
    ? intl.formatDate(lastAt, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' })
    : null

  return (
    <section
      aria-label={t('memory.dream.title')}
      data-testid="memory-dream-summary"
      className="bg-surface-container-lowest rounded-xl border border-outline-variant/30 p-md shadow-e1"
    >
      <div className="flex items-center gap-xs mb-sm">
        <span className="material-symbols-outlined icon-md text-primary" aria-hidden="true">bedtime</span>
        <h2 className="text-label-lg font-semibold text-on-surface">{t('memory.dream.title')}</h2>
        {time && <span className="ml-auto text-label-xs text-on-surface-variant tabular-nums">{time}</span>}
      </div>

      {!hasTotals && !newestLine && !time ? (
        <p className="text-label-sm text-on-surface-variant py-xs">{t('memory.dream.sideEmpty')}</p>
      ) : (
        <div className="space-y-xs mb-sm">
          {hasTotals && (
            <div className="flex items-center gap-sm text-label-sm text-on-surface-variant" data-testid="memory-dream-totals">
              {(['merge', 'remove', 'add'] as const).map(kind => totals[kind] > 0 ? (
                <span key={kind} className="inline-flex items-center gap-[2px] tabular-nums">
                  <span
                    className={cn(
                      'material-symbols-outlined icon-xs',
                      kind === 'remove' ? 'text-error' : kind === 'add' ? 'text-primary' : 'text-on-surface-variant',
                    )}
                    aria-hidden="true"
                  >
                    {kind === 'merge' ? 'call_merge' : kind === 'remove' ? 'delete' : 'add_circle'}
                  </span>
                  {totals[kind]}
                </span>
              ) : null)}
            </div>
          )}
          {newestLine && (
            <div className="flex items-start gap-xs text-label-sm text-on-surface-variant min-w-0">
              <span className="material-symbols-outlined icon-xs mt-[2px] shrink-0" aria-hidden="true">folder</span>
              <span className="min-w-0 truncate">{newestLine}</span>
            </div>
          )}
          {!hasTotals && !newestLine && time && (
            <div className="text-label-sm text-on-surface-variant">
              {t('memory.dream.lastDistilledNoStats', { time })}
            </div>
          )}
        </div>
      )}

      <button
        type="button"
        onClick={() => document.getElementById('dream-panel')?.scrollIntoView()}
        data-testid="memory-dream-view-changes"
        className="inline-flex items-center gap-xs text-label-sm text-link hover:underline cursor-pointer focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary rounded-sm"
      >
        <span className="material-symbols-outlined icon-sm" aria-hidden="true">history</span>
        {t('memory.dream.viewChanges')}
      </button>
    </section>
  )
}
