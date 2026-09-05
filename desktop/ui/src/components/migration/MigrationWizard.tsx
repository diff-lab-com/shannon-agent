// Migration wizard (P1-6) — import assets from a Claude Code / ZCode install.
//
// A self-contained dialog reused from two entry points: the Welcome flow's
// final step and Settings → General. The internal phase machine mirrors the
// brief's funnel: pick a source → scan → review (grouped checklist with
// conflict badges + expandable preview) → apply → result summary. All Rust
// side effects go through the frozen `migration_scan` / `migration_preview` /
// `migration_apply` contract (tauri-api.ts).
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import { Spinner } from '@/components/ui/loading-state'
import * as api from '@/lib/tauri-api'
import type {
  MigrationAsset,
  MigrationItemInput,
  MigrationPreviewItem,
  MigrationScanResult,
  MigrationSourceId,
} from '@/lib/tauri-api'

type Phase = 'source' | 'scanning' | 'review' | 'applying' | 'result'

// Stable display order for the kind groups.
const KIND_ORDER: api.MigrationAssetKind[] = ['mcp', 'skill', 'command', 'memory', 'settings-rules']

const SOURCES: { id: MigrationSourceId; labelKey: string; descKey: string; icon: string }[] = [
  {
    id: 'claude-code',
    labelKey: 'welcome.migration.source.claudeCode',
    descKey: 'welcome.migration.source.claudeCodeDesc',
    icon: 'terminal',
  },
  {
    id: 'zcode',
    labelKey: 'welcome.migration.source.zcode',
    descKey: 'welcome.migration.source.zcodeDesc',
    icon: 'smart_toy',
  },
]

interface MigrationWizardProps {
  open: boolean
  onClose: () => void
  /** Test seam — defaults to the real tauri-api wrappers. */
  apiOverride?: typeof api
}

export default function MigrationWizard({ open, onClose, apiOverride }: MigrationWizardProps) {
  const intl = useIntl()
  const backend = apiOverride ?? api

  const [phase, setPhase] = useState<Phase>('source')
  const [source, setSource] = useState<MigrationSourceId | null>(null)
  const [scan, setScan] = useState<MigrationScanResult | null>(null)
  const [previews, setPreviews] = useState<Record<string, string>>({})
  const [selected, setSelected] = useState<Record<string, boolean>>({})
  const [expanded, setExpanded] = useState<Record<string, boolean>>({})
  // Per-item choice for existing, differing targets (rename is the safe default).
  const [conflictChoices, setConflictChoices] = useState<Record<string, 'overwrite' | 'rename' | 'skip'>>({})
  const [report, setReport] = useState<api.MigrationApplyReport | null>(null)

  const dialogRef = useRef<HTMLDivElement>(null)
  const requestIdRef = useRef(0)

  // Reset whenever the dialog closes so reopening starts at the source step.
  useEffect(() => {
    if (!open) {
      setPhase('source')
      setSource(null)
      setScan(null)
      setPreviews({})
      setSelected({})
      setExpanded({})
      setConflictChoices({})
      setReport(null)
    }
  }, [open])

  // Focus the dialog on open + Escape closes (basic a11y: the dialog is
  // reachable by keyboard and dismissible without a pointer).
  useEffect(() => {
    if (open) dialogRef.current?.focus()
  }, [open])

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === 'Escape' && phase !== 'scanning' && phase !== 'applying') {
        e.stopPropagation()
        onClose()
      }
    },
    [onClose, phase],
  )

  const runScan = useCallback(
    async (src: MigrationSourceId) => {
      const requestId = ++requestIdRef.current
      setSource(src)
      setPhase('scanning')
      try {
        const result = await backend.migrationScan(src)
        if (requestIdRef.current !== requestId) return // superseded
        setScan(result)
        const initialSelected: Record<string, boolean> = {}
        for (const asset of result.items) initialSelected[asset.id] = true
        setSelected(initialSelected)
        if (result.items.length === 0) {
          setPhase('review') // empty state renders inside the review step
          return
        }
        // Fetch per-item diff summaries up front so conflict rows can expand
        // without another round-trip.
        const inputs: MigrationItemInput[] = result.items.map(a => ({
          id: a.id,
          action: 'import',
        }))
        try {
          const preview = await backend.migrationPreview(src, inputs)
          if (requestIdRef.current !== requestId) return
          const map: Record<string, string> = {}
          for (const row of preview.perItem as MigrationPreviewItem[]) map[row.id] = row.diffSummary
          setPreviews(map)
        } catch (e) {
          // Preview is advisory — a failure must not block the review list.
          console.warn('migrationPreview failed:', e)
        }
        setPhase('review')
      } catch (e) {
        if (requestIdRef.current !== requestId) return
        console.error('migrationScan failed:', e)
        setScan({ source: src, items: [], notFound: [], errors: [] })
        setPhase('review')
      }
    },
    [backend],
  )

  const toggle = (id: string) => setSelected(prev => ({ ...prev, [id]: !prev[id] }))

  const selectedCount = useMemo(
    () => (scan?.items ?? []).filter(a => selected[a.id]).length,
    [scan, selected],
  )

  const grouped = useMemo(() => {
    const groups = new Map<api.MigrationAssetKind, MigrationAsset[]>()
    for (const kind of KIND_ORDER) groups.set(kind, [])
    for (const asset of scan?.items ?? []) {
      const list = groups.get(asset.kind)
      if (list) list.push(asset)
    }
    return [...groups.entries()].filter(([, list]) => list.length > 0)
  }, [scan])

  const runApply = useCallback(async () => {
    if (!source) return
    const requestId = ++requestIdRef.current
    setPhase('applying')
    const inputs: MigrationItemInput[] = (scan?.items ?? [])
      .filter(a => selected[a.id])
      .map(a => ({
        id: a.id,
        action: 'import' as const,
        ...(a.conflict === 'overwrite'
          ? { conflict: conflictChoices[a.id] ?? ('rename' as const) }
          : {}),
      }))
    try {
      const result = await backend.migrationApply(source, inputs)
      if (requestIdRef.current !== requestId) return
      setReport(result)
    } catch (e) {
      if (requestIdRef.current !== requestId) return
      // Total failure (command rejected) still needs a visible end state.
      setReport({
        imported: 0,
        skipped: 0,
        failed: [{ id: '*', error: e instanceof Error ? e.message : String(e) }],
      })
    } finally {
      if (requestIdRef.current === requestId) setPhase('result')
    }
  }, [backend, conflictChoices, scan, selected, source])

  if (!open) return null

  const t = (id: string, values?: Record<string, string | number>) =>
    intl.formatMessage({ id }, values)

  return (
    <div
      className="fixed inset-0 z-50 bg-black/50 flex items-center justify-center p-lg"
      data-testid="migration-wizard-backdrop"
      onClick={(e) => {
        if (e.target === e.currentTarget && phase !== 'scanning' && phase !== 'applying') onClose()
      }}
    >
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-label={t('welcome.migration.title')}
        tabIndex={-1}
        onKeyDown={handleKeyDown}
        data-testid="migration-wizard"
        className="bg-surface-container-lowest border border-outline-variant/30 rounded-2xl shadow-lg w-full max-w-2xl max-h-[85vh] flex flex-col outline-none p-xl"
      >
        <header className="flex items-start justify-between mb-lg">
          <div>
            <h2 className="font-headline-lg text-on-surface">{t('welcome.migration.title')}</h2>
            <p className="font-body-sm text-on-surface-variant mt-xs">
              {t('welcome.migration.subtitle')}
            </p>
          </div>
          <Button
            variant="ghost"
            onClick={onClose}
            disabled={phase === 'scanning' || phase === 'applying'}
            aria-label={t('welcome.migration.close')}
            className="text-on-surface-variant hover:text-primary cursor-pointer rounded px-xs"
          >
            <span className="material-symbols-outlined text-[20px]" aria-hidden="true">close</span>
          </Button>
        </header>

        <div className="flex-1 overflow-y-auto min-h-0" aria-live="polite">
          {phase === 'source' && (
            <div role="radiogroup" aria-label={t('welcome.migration.source.label')} className="space-y-md">
              {SOURCES.map(s => (
                <button
                  key={s.id}
                  type="button"
                  role="radio"
                  aria-checked={source === s.id}
                  onClick={() => runScan(s.id)}
                  data-testid={`migration-source-${s.id}`}
                  className="w-full text-left flex items-center gap-md p-lg rounded-xl border border-outline-variant/50 bg-surface-container-low hover:border-primary/60 hover:bg-surface-container cursor-pointer transition-all focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
                >
                  <span className="material-symbols-outlined text-primary text-[28px]" aria-hidden="true">{s.icon}</span>
                  <span>
                    <span className="block font-headline-md text-on-surface">{t(s.labelKey)}</span>
                    <span className="block font-body-sm text-on-surface-variant mt-xs">{t(s.descKey)}</span>
                  </span>
                </button>
              ))}
            </div>
          )}

          {phase === 'scanning' && (
            <div className="flex flex-col items-center gap-md py-2xl" data-testid="migration-scanning">
              <Spinner className="text-primary text-[28px]" />
              <p className="font-body-md text-on-surface-variant">
                {t('welcome.migration.scan.running', { source: source ?? '' })}
              </p>
            </div>
          )}

          {phase === 'review' && scan && (
            <div data-testid="migration-review">
              {scan.items.length === 0 ? (
                <p className="font-body-md text-on-surface-variant py-xl text-center">
                  {t('welcome.migration.scan.empty', { source: source ?? '' })}
                </p>
              ) : (
                <>
                  <div className="flex items-center justify-between mb-md">
                    <h3 className="font-headline-md text-on-surface">{t('welcome.migration.review.title')}</h3>
                    <div className="flex gap-xs">
                      <Button
                        variant="ghost"
                        onClick={() => {
                          const next: Record<string, boolean> = {}
                          for (const a of scan.items) next[a.id] = true
                          setSelected(next)
                        }}
                        className="font-label-sm text-primary cursor-pointer px-sm"
                      >
                        {t('welcome.migration.review.selectAll')}
                      </Button>
                      <Button
                        variant="ghost"
                        onClick={() => {
                          const next: Record<string, boolean> = {}
                          for (const a of scan.items) next[a.id] = false
                          setSelected(next)
                        }}
                        className="font-label-sm text-on-surface-variant cursor-pointer px-sm"
                      >
                        {t('welcome.migration.review.clearAll')}
                      </Button>
                    </div>
                  </div>

                  {grouped.map(([kind, assets]) => (
                    <section key={kind} className="mb-md" aria-label={t(`welcome.migration.kind.${kind}`)}>
                      <h4 className="font-label-md text-on-surface-variant mb-xs">{t(`welcome.migration.kind.${kind}`)}</h4>
                      <ul className="rounded-xl border border-outline-variant/40 divide-y divide-outline-variant/30 overflow-hidden">
                        {assets.map(asset => {
                          const isExpanded = !!expanded[asset.id]
                          const badge = asset.conflict
                          return (
                            <li key={asset.id} className="bg-surface-container-lowest">
                              <div className="flex items-center gap-sm px-md py-sm">
                                <input
                                  type="checkbox"
                                  id={`migration-item-${asset.id}`}
                                  checked={!!selected[asset.id]}
                                  onChange={() => toggle(asset.id)}
                                  className="accent-primary"
                                  aria-label={t('welcome.migration.review.itemAria', {
                                    name: asset.name,
                                    kind: t(`welcome.migration.kind.${kind}`),
                                  })}
                                  data-testid={`migration-check-${asset.id}`}
                                />
                                <label
                                  htmlFor={`migration-item-${asset.id}`}
                                  className="flex-1 font-body-sm text-on-surface cursor-pointer"
                                >
                                  {asset.name}
                                </label>
                                <span
                                  data-testid={`migration-badge-${asset.id}`}
                                  className={
                                    'font-label-sm px-sm py-0.5 rounded-full ' +
                                    (badge === 'none'
                                      ? 'bg-primary-container text-on-primary-container'
                                      : badge === 'overwrite'
                                        ? 'bg-error-container text-on-error-container'
                                        : 'bg-surface-container-high text-on-surface-variant')
                                  }
                                >
                                  {t(`welcome.migration.badge.${badge}`)}
                                </span>
                                {asset.conflict !== 'none' && previews[asset.id] && (
                                  <button
                                    type="button"
                                    onClick={() => setExpanded(prev => ({ ...prev, [asset.id]: !isExpanded }))}
                                    aria-expanded={isExpanded}
                                    aria-label={t('welcome.migration.review.expandAria', { name: asset.name })}
                                    data-testid={`migration-expand-${asset.id}`}
                                    className="text-on-surface-variant hover:text-primary cursor-pointer rounded px-xs focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
                                  >
                                    <span className="material-symbols-outlined text-[18px]" aria-hidden="true">
                                      {isExpanded ? 'expand_less' : 'expand_more'}
                                    </span>
                                  </button>
                                )}
                              </div>
                              {isExpanded && previews[asset.id] && (
                                <p
                                  data-testid={`migration-preview-${asset.id}`}
                                  className="px-md pb-sm font-body-sm text-on-surface-variant whitespace-pre-line"
                                >
                                  {previews[asset.id]}
                                </p>
                              )}
                              {isExpanded && asset.conflict === 'overwrite' && (
                                <div className="px-md pb-sm flex items-center gap-sm">
                                  <label
                                    htmlFor={`migration-conflict-${asset.id}`}
                                    className="font-label-sm text-on-surface-variant"
                                  >
                                    {t('welcome.migration.review.conflictChoice')}
                                  </label>
                                  <select
                                    id={`migration-conflict-${asset.id}`}
                                    value={conflictChoices[asset.id] ?? 'rename'}
                                    onChange={e =>
                                      setConflictChoices(prev => ({
                                        ...prev,
                                        [asset.id]: e.target.value as 'overwrite' | 'rename' | 'skip',
                                      }))
                                    }
                                    data-testid={`migration-conflict-${asset.id}`}
                                    className="font-label-sm bg-surface-container-low border border-outline-variant/50 rounded px-xs py-0.5 text-on-surface cursor-pointer"
                                  >
                                    <option value="rename">{t('welcome.migration.conflict.rename')}</option>
                                    <option value="overwrite">{t('welcome.migration.conflict.overwrite')}</option>
                                    <option value="skip">{t('welcome.migration.conflict.skip')}</option>
                                  </select>
                                </div>
                              )}
                            </li>
                          )
                        })}
                      </ul>
                    </section>
                  ))}

                  {(scan.notFound.length > 0 || scan.errors.length > 0) && (
                    <details className="mt-md" data-testid="migration-notfound">
                      <summary className="font-label-md text-on-surface-variant cursor-pointer">
                        {t('welcome.migration.review.notFound')}
                      </summary>
                      <ul className="font-body-sm text-on-surface-variant mt-xs space-y-0.5">
                        {scan.notFound.map(slot => (
                          <li key={slot}>· {slot}</li>
                        ))}
                        {scan.errors.map(err => (
                          <li key={`${err.path}:${err.error}`}>· {err.path} — {err.error}</li>
                        ))}
                      </ul>
                    </details>
                  )}

                  <p className="font-body-sm text-on-surface-variant mt-md">
                    {t('welcome.migration.review.unverified')}
                  </p>
                </>
              )}
            </div>
          )}

          {phase === 'applying' && (
            <div className="flex flex-col items-center gap-md py-2xl" data-testid="migration-applying">
              <Spinner className="text-primary text-[28px]" />
              <p className="font-body-md text-on-surface-variant">{t('welcome.migration.apply.running')}</p>
            </div>
          )}

          {phase === 'result' && report && (
            <div data-testid="migration-result" className="space-y-md">
              <h3 className="font-headline-md text-on-surface">{t('welcome.migration.result.title')}</h3>
              <div
                role="status"
                aria-live="polite"
                aria-label={t('welcome.migration.result.aria', {
                  imported: report.imported,
                  skipped: report.skipped,
                  failed: report.failed.length,
                })}
                className="flex gap-lg font-body-md text-on-surface bg-surface-container-low rounded-xl p-md"
              >
                <span className="text-green-700 dark:text-green-400">
                  ✓ {t('welcome.migration.result.imported', { count: report.imported })}
                </span>
                <span className="text-on-surface-variant">
                  {t('welcome.migration.result.skipped', { count: report.skipped })}
                </span>
                {report.failed.length > 0 && (
                  <span className="text-error">
                    {t('welcome.migration.result.failed', { count: report.failed.length })}
                  </span>
                )}
              </div>
              {report.failed.length > 0 && (
                <ul className="rounded-xl border border-error/40 bg-error-container/30 p-md space-y-xs" data-testid="migration-result-failures">
                  {report.failed.map(f => (
                    <li key={f.id} className="font-body-sm text-on-surface">
                      <span className="font-label-sm text-on-surface-variant">{f.id}</span> — {f.error}
                    </li>
                  ))}
                </ul>
              )}
              <p className="font-body-sm text-on-surface-variant">{t('welcome.migration.result.unverified')}</p>
            </div>
          )}
        </div>

        <footer className="flex justify-between items-center mt-xl">
          <Button
            variant="ghost"
            onClick={onClose}
            disabled={phase === 'scanning' || phase === 'applying'}
            className="font-label-md text-on-surface-variant hover:text-primary cursor-pointer rounded"
          >
            {t('welcome.migration.later')}
          </Button>
          <div className="flex gap-sm">
            {phase === 'review' && scan && scan.items.length > 0 && (
              <>
                <Button
                  variant="ghost"
                  onClick={() => setPhase('source')}
                  className="font-label-md text-on-surface-variant hover:text-primary cursor-pointer rounded"
                >
                  {t('welcome.migration.back')}
                </Button>
                <Button
                  onClick={runApply}
                  disabled={selectedCount === 0}
                  data-testid="migration-apply"
                  className="px-lg py-sm bg-primary text-on-primary rounded-lg font-label-md cursor-pointer hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
                >
                  {t('welcome.migration.review.import', { count: selectedCount })}
                </Button>
              </>
            )}
            {phase === 'result' && (
              <Button
                onClick={onClose}
                data-testid="migration-done"
                className="px-lg py-sm bg-primary text-on-primary rounded-lg font-label-md cursor-pointer hover:bg-primary/90 focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
              >
                {t('welcome.migration.result.done')}
              </Button>
            )}
          </div>
        </footer>
      </div>
    </div>
  )
}
