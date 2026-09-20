// Memory panel — full CRUD UI for the persistent memory layer (P2.1).
//
// SCOPE: browse / search / create / edit / delete MemoryEntry records backed
// by shannon_core::memory::MemoryStore at ~/.shannon/memories/.
//
// Layout: stats header → filter row → list of memory cards → inline editor
// drawer for create/edit. All mutations go through the Tauri commands and
// re-fetch the visible list on success so the UI stays in sync with disk.
//
// T3.1 — this file is now the orchestrator only. Sub-components live next to
// it: constants.ts (lookup tables), StatCard.tsx, MemoryCard.tsx,
// MemoryEditor.tsx (with its own Field.tsx helper).

import { useCallback, useEffect, useState } from 'react'
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import { toast } from 'sonner'
import {
  createMemory,
  deleteMemory,
  getMemoryGraph,
  getMemoryStats,
  listMemories,
  listMemoryProjects,
  updateMemory,
  type MemoryEntry,
  type MemoryGraph,
  type MemoryStats,
} from '@/lib/tauri-api'
import { CATEGORIES, type CategoryFilter } from './constants'
import { MemoryCard } from './MemoryCard'
import { MemoryEditor, type MemorySaveInput } from './MemoryEditor'
import { MemoryGraphView } from './MemoryGraphView'
import StatCard from '@/components/ui/stat-card'
import { cn } from '@/lib/utils'

type MemoryView = 'list' | 'graph'

export default function MemoryPanel({
  onOpenMemorySource,
}: {
  onOpenMemorySource?: (memoryId: string, sourceSessionId: string) => void
}) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  const [view, setView] = useState<MemoryView>('list')
  const [entries, setEntries] = useState<MemoryEntry[]>([])
  const [projects, setProjects] = useState<string[]>([])
  const [stats, setStats] = useState<MemoryStats | null>(null)
  const [graph, setGraph] = useState<MemoryGraph | null>(null)
  const [graphLoading, setGraphLoading] = useState(false)
  const [loading, setLoading] = useState(true)
  const [errorMsg, setErrorMsg] = useState<string | null>(null)

  const [projectFilter, setProjectFilter] = useState<string>('all')
  const [categoryFilter, setCategoryFilter] = useState<CategoryFilter>('all')
  // 2026-09 review: typing used to fire an IPC round-trip on every
  // keystroke. We split the live query (instant UI feedback) from the
  // backend-bound query (debounced 250 ms).
  const [query, setQuery] = useState('')
  const [debouncedQuery, setDebouncedQuery] = useState('')
  const [pendingDeleteId, setPendingDeleteId] = useState<string | null>(null)

  const [editing, setEditing] = useState<MemoryEntry | null>(null)
  const [creating, setCreating] = useState(false)

  useEffect(() => {
    const id = window.setTimeout(() => setDebouncedQuery(query), 250)
    return () => window.clearTimeout(id)
  }, [query])

  const fetchAll = useCallback(async () => {
    setLoading(true)
    setErrorMsg(null)
    try {
      const [rows, projs, s] = await Promise.all([
        listMemories({
          project: projectFilter === 'all' ? null : projectFilter,
          category: categoryFilter === 'all' ? null : categoryFilter,
          query: debouncedQuery.trim() || null,
        }),
        listMemoryProjects(),
        getMemoryStats(),
      ])
      setEntries(rows)
      setProjects(projs)
      setStats(s)
    } catch (e) {
      setErrorMsg(e instanceof Error ? e.message : String(e))
    } finally {
      setLoading(false)
    }
  }, [projectFilter, categoryFilter, debouncedQuery])

  useEffect(() => {
    void fetchAll()
  }, [fetchAll])

  // P2-4: graph payload is fetched when the graph tab is opened (and refetched
  // when the project filter changes — category/query narrow client-side).
  useEffect(() => {
    if (view !== 'graph') return
    let cancelled = false
    setGraphLoading(true)
    getMemoryGraph(projectFilter === 'all' ? null : projectFilter)
      .then((g) => {
        if (!cancelled) setGraph(g)
      })
      .catch((e) => {
        if (!cancelled) setErrorMsg(e instanceof Error ? e.message : String(e))
      })
      .finally(() => {
        if (!cancelled) setGraphLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [view, projectFilter])

  const handleDelete = (id: string) => setPendingDeleteId(id)

  const confirmDelete = async () => {
    const id = pendingDeleteId
    if (!id) return
    try {
      const ok = await deleteMemory(id)
      if (!ok) {
        toast.error(t('memory.toast.notFound'))
        return
      }
      toast.success(t('memory.toast.deleted'))
      await fetchAll()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : t('memory.toast.failedDelete'))
    } finally {
      setPendingDeleteId(null)
    }
  }

  const handleSave = async (input: MemorySaveInput) => {
    try {
      if (input.id) {
        await updateMemory({
          id: input.id,
          content: input.content,
          tags: input.tags,
          category: input.category,
        })
        toast.success(t('memory.toast.updated'))
      } else {
        await createMemory({
          project: input.project,
          category: input.category,
          content: input.content,
          tags: input.tags,
          confidence: input.confidence,
        })
        toast.success(t('memory.toast.created'))
      }
      setEditing(null)
      setCreating(false)
      await fetchAll()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : t('memory.toast.failedSave'))
    }
  }

  const filteredCount = entries.length
  const isEmpty = !loading && entries.length === 0

  return (
    <div className="flex-1 overflow-y-auto w-full pb-16">
      <div className="max-w-[1100px] mx-auto px-lg py-xl">
        <p className="text-body-md text-on-surface-variant mb-xl">
          {t('memory.subtitle')}
        </p>

        {stats && (
          <div className="grid grid-cols-2 md:grid-cols-5 gap-md mb-xl">
            <StatCard label={t('memory.stats.total')} value={stats.total} icon="database" />
            <StatCard
              label={t('memory.stats.preferences')}
              value={stats.by_category['preference'] ?? 0}
              icon="tune"
            />
            <StatCard
              label={t('memory.stats.decisions')}
              value={stats.by_category['decision'] ?? 0}
              icon="fork_right"
            />
            <StatCard
              label={t('memory.stats.errors')}
              value={stats.by_category['error'] ?? 0}
              icon="bug_report"
            />
            <StatCard
              label={t('memory.stats.projects')}
              value={Object.keys(stats.by_project).length}
              icon="folder"
            />
          </div>
        )}

        {errorMsg && (
          <div className="flex items-center gap-sm px-md py-sm rounded-xl bg-error/10 border border-error/20 text-error font-label-md mb-lg">
            <span className="material-symbols-outlined text-[18px]">error</span>
            {errorMsg}
            <Button
              variant="ghost"
              size="icon-sm"
              className="ml-auto text-error/60 hover:text-error"
              onClick={() => setErrorMsg(null)}
            >
              <span className="material-symbols-outlined text-[18px]">close</span>
            </Button>
          </div>
        )}

        <div className="flex flex-wrap items-center gap-md mb-lg">
          {/* P2-4: list/graph view switch — the list view stays the
              accessible alternative to the svg graph. */}
          <div
            role="tablist"
            aria-label={t('memory.view.aria')}
            className="flex rounded-xl border border-outline-variant bg-surface-container-low p-[2px]"
          >
            {(['list', 'graph'] as const).map((v) => (
              <button
                key={v}
                role="tab"
                aria-selected={view === v}
                onClick={() => setView(v)}
                className={cn(
                  'px-md py-sm rounded-lg text-label-md font-bold transition-colors cursor-pointer',
                  view === v
                    ? 'bg-surface-container-lowest text-on-surface shadow-sm'
                    : 'text-on-surface-variant hover:text-on-surface',
                )}
              >
                <span className="material-symbols-outlined text-[16px] align-middle mr-xs">
                  {v === 'list' ? 'list' : 'hub'}
                </span>
                {t(`memory.view.${v}`)}
              </button>
            ))}
          </div>

          <select
            value={projectFilter}
            onChange={(e) => setProjectFilter(e.target.value)}
            aria-label={t('memory.filter.projectAria')}
            className="px-md py-sm rounded-xl bg-surface-container-low border border-outline-variant text-label-md transition-colors hover:border-primary/30 focus-visible:border-primary focus-visible:ring-2 focus-visible:ring-primary/20 focus-visible:outline-none cursor-pointer"
          >
            <option value="all">{t('memory.filter.allProjects')}</option>
            {projects.map((p) => (
              <option key={p} value={p}>
                {p}
              </option>
            ))}
          </select>

          <select
            value={categoryFilter}
            onChange={(e) => setCategoryFilter(e.target.value as CategoryFilter)}
            aria-label={t('memory.filter.categoryAria')}
            className="px-md py-sm rounded-xl bg-surface-container-low border border-outline-variant text-label-md transition-colors hover:border-primary/30 focus-visible:border-primary focus-visible:ring-2 focus-visible:ring-primary/20 focus-visible:outline-none cursor-pointer"
          >
            {CATEGORIES.map((c) => (
              <option key={c} value={c}>
                {t(`memory.category.${c}`)}
              </option>
            ))}
          </select>

          <div className="flex-1 min-w-[200px] relative">
            <span className="material-symbols-outlined absolute left-md top-1/2 -translate-y-1/2 text-on-surface-variant text-[18px]">
              search
            </span>
            <input
              type="text"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder={t('memory.searchPlaceholder')}
              className="w-full pl-[40px] pr-md py-sm rounded-xl bg-surface-container-low border border-outline-variant text-label-md transition-colors hover:border-primary/30 focus-visible:border-primary focus-visible:ring-2 focus-visible:ring-primary/20 focus-visible:outline-none"
            />
          </div>

          <Button
            onClick={() => setCreating(true)}
            className="gap-xs px-md py-sm text-[14px] font-bold"
          >
            <span className="material-symbols-outlined text-[18px]">add</span>
            {t('memory.action.create')}
          </Button>
        </div>

        {/* Audit §P2-2 (round 6): suppress the count line during a reload so
            "暂无记忆" + "正在加载记忆…" never appear at the same time. The
            loading block (below) is the single source of truth while
            loading is true. */}
        {!loading && (
          <div className="text-label-sm text-on-surface-variant mb-md">
            {intl.formatMessage({ id: 'memory.listCount' }, { count: filteredCount })}
          </div>
        )}

        {view === 'graph' ? (
          <MemoryGraphView
            graph={graph ?? { project: null, nodes: [], edges: [], entryCount: 0, maxEntries: 200, truncated: false }}
            loading={loading || graphLoading}
            category={categoryFilter}
            query={query}
            onOpenMemorySource={onOpenMemorySource}
          />
        ) : loading ? (
          <div className="text-center py-3xl text-on-surface-variant">
            {t('memory.loading')}
          </div>
        ) : isEmpty ? (
          <div className="flex flex-col items-center justify-center text-center py-3xl px-lg">
            <div className="w-20 h-20 rounded-2xl bg-primary-container/30 flex items-center justify-center mb-lg">
              <span className="material-symbols-outlined text-[40px] text-primary" aria-hidden="true">
                psychology
              </span>
            </div>
            <h2 className="font-headline-sm text-on-surface mb-xs">{t('memory.empty.title')}</h2>
            <p className="text-on-surface-variant mb-lg max-w-md">{t('memory.empty.desc')}</p>
            <Button
              onClick={() => setCreating(true)}
              className="gap-xs px-md py-sm text-[14px] font-bold bg-primary text-on-primary"
            >
              <span className="material-symbols-outlined text-[18px]">add</span>
              {t('memory.action.createFirst')}
            </Button>
          </div>
        ) : (
          <div className="space-y-md">
            {entries.map((entry) => (
              <MemoryCard
                key={entry.id}
                entry={entry}
                onEdit={() => setEditing(entry)}
                onDelete={() => handleDelete(entry.id)}
                onOpenMemorySource={onOpenMemorySource}
              />
            ))}
          </div>
        )}
      </div>

      {(creating || editing) && (
        <MemoryEditor
          initial={editing}
          onCancel={() => {
            setCreating(false)
            setEditing(null)
          }}
          onSave={handleSave}
        />
      )}

      <ConfirmDialog
        open={pendingDeleteId !== null}
        title={t('memory.confirmDelete.title')}
        message={t('memory.confirmDelete.message')}
        confirmLabel={t('memory.confirmDelete.confirm')}
        cancelLabel={t('memory.confirmDelete.cancel')}
        destructive
        onConfirm={() => void confirmDelete()}
        onCancel={() => setPendingDeleteId(null)}
      />
    </div>
  )
}
