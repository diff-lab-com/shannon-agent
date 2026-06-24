// Memory panel — full CRUD UI for the persistent memory layer (P2.1).
//
// SCOPE: browse / search / create / edit / delete MemoryEntry records backed
// by shannon_core::memory::MemoryStore at ~/.shannon/memories/.
//
// Layout: stats header → filter row → list of memory cards → inline editor
// drawer for create/edit. All mutations go through the Tauri commands and
// re-fetch the visible list on success so the UI stays in sync with disk.

import { useCallback, useEffect, useState } from 'react'
import type React from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import {
  createMemory,
  deleteMemory,
  getMemoryStats,
  listMemories,
  listMemoryProjects,
  updateMemory,
  type MemoryCategory,
  type MemoryEntry,
  type MemoryStats,
} from '@/lib/tauri-api'

type CategoryFilter = MemoryCategory | 'all'

const CATEGORIES: CategoryFilter[] = ['all', 'preference', 'pattern', 'decision', 'error', 'context']

const CATEGORY_ICON: Record<MemoryCategory, string> = {
  preference: 'tune',
  pattern: 'pattern',
  decision: 'fork_right',
  error: 'bug_report',
  context: 'lightbulb',
}

const CATEGORY_COLOR: Record<MemoryCategory, string> = {
  preference: 'bg-primary-container/50 text-on-primary-container',
  pattern: 'bg-secondary-container/50 text-on-secondary-container',
  decision: 'bg-tertiary-container/50 text-on-tertiary-container',
  error: 'bg-error-container/50 text-on-error-container',
  context: 'bg-surface-container-high text-on-surface',
}

export default function MemoryPanel() {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  const [entries, setEntries] = useState<MemoryEntry[]>([])
  const [projects, setProjects] = useState<string[]>([])
  const [stats, setStats] = useState<MemoryStats | null>(null)
  const [loading, setLoading] = useState(true)
  const [errorMsg, setErrorMsg] = useState<string | null>(null)

  const [projectFilter, setProjectFilter] = useState<string>('all')
  const [categoryFilter, setCategoryFilter] = useState<CategoryFilter>('all')
  const [query, setQuery] = useState('')

  const [editing, setEditing] = useState<MemoryEntry | null>(null)
  const [creating, setCreating] = useState(false)

  const fetchAll = useCallback(async () => {
    setLoading(true)
    setErrorMsg(null)
    try {
      const [rows, projs, s] = await Promise.all([
        listMemories({
          project: projectFilter === 'all' ? null : projectFilter,
          category: categoryFilter === 'all' ? null : categoryFilter,
          query: query.trim() || null,
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
  }, [projectFilter, categoryFilter, query])

  useEffect(() => {
    void fetchAll()
  }, [fetchAll])

  const handleDelete = async (id: string) => {
    if (!window.confirm(t('memory.confirmDelete'))) return
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
    }
  }

  const handleSave = async (input: {
    id?: string
    project: string
    category: MemoryCategory
    content: string
    tags: string[]
    confidence: number
  }) => {
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
        <header className="mb-xl">
          <h1 className="text-headline-md font-bold text-on-surface mb-xs">
            {t('memory.title')}
          </h1>
          <p className="text-body-md text-on-surface-variant">
            {t('memory.subtitle')}
          </p>
        </header>

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
            <button
              className="ml-auto text-error/60 hover:text-error cursor-pointer"
              onClick={() => setErrorMsg(null)}
            >
              <span className="material-symbols-outlined text-[18px]">close</span>
            </button>
          </div>
        )}

        <div className="flex flex-wrap items-center gap-md mb-lg">
          <select
            value={projectFilter}
            onChange={(e) => setProjectFilter(e.target.value)}
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

          <button
            onClick={() => setCreating(true)}
            className="inline-flex items-center gap-xs px-md py-sm rounded-xl bg-primary text-on-primary text-label-md font-bold hover:brightness-110"
          >
            <span className="material-symbols-outlined text-[18px]">add</span>
            {t('memory.action.create')}
          </button>
        </div>

        <div className="text-label-sm text-on-surface-variant mb-md">
          {intl.formatMessage({ id: 'memory.listCount' }, { count: filteredCount })}
        </div>

        {loading ? (
          <div className="text-center py-3xl text-on-surface-variant">
            {t('memory.loading')}
          </div>
        ) : isEmpty ? (
          <div className="text-center py-3xl">
            <span className="material-symbols-outlined text-[48px] text-on-surface-variant/40 mb-md block">
              psychology
            </span>
            <p className="text-on-surface-variant mb-lg">{t('memory.empty')}</p>
            <button
              onClick={() => setCreating(true)}
              className="inline-flex items-center gap-xs px-md py-sm rounded-xl bg-primary text-on-primary text-label-md font-bold"
            >
              <span className="material-symbols-outlined text-[18px]">add</span>
              {t('memory.action.createFirst')}
            </button>
          </div>
        ) : (
          <div className="space-y-md">
            {entries.map((entry) => (
              <MemoryCard
                key={entry.id}
                entry={entry}
                onEdit={() => setEditing(entry)}
                onDelete={() => handleDelete(entry.id)}
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
    </div>
  )
}

function StatCard({ label, value, icon }: { label: string; value: number; icon: string }) {
  return (
    <div className="flex items-center gap-sm px-md py-md rounded-xl bg-surface-container-low border border-outline-variant/30">
      <span className="material-symbols-outlined text-primary text-[24px]">{icon}</span>
      <div>
        <div className="text-label-lg font-bold text-on-surface leading-none">{value}</div>
        <div className="text-label-xs text-on-surface-variant mt-[2px]">{label}</div>
      </div>
    </div>
  )
}

function MemoryCard({
  entry,
  onEdit,
  onDelete,
}: {
  entry: MemoryEntry
  onEdit: () => void
  onDelete: () => void
}) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const fmtDate = (iso: string) => {
    const d = new Date(iso)
    if (Number.isNaN(d.getTime())) return iso
    return intl.formatDate(d, { year: 'numeric', month: 'short', day: 'numeric' })
  }

  return (
    <div className="px-md py-md rounded-xl bg-surface-container-low border border-outline-variant/30 shadow-sm hover:shadow-md hover:border-primary/30 transition-all">
      <div className="flex items-start gap-md">
        <span
          className={`material-symbols-outlined text-[20px] mt-[2px] px-sm py-xs rounded-lg ${CATEGORY_COLOR[entry.category]}`}
        >
          {CATEGORY_ICON[entry.category]}
        </span>
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-sm mb-xs">
            <span className="text-label-xs px-sm py-[2px] rounded-full bg-surface-container-high text-on-surface-variant font-bold uppercase">
              {entry.category}
            </span>
            <span className="text-label-xs text-on-surface-variant">{entry.project}</span>
            <span className="text-label-xs text-on-surface-variant/60">
              · {fmtDate(entry.created_at)}
            </span>
            {entry.access_count > 0 && (
              <span className="text-label-xs text-on-surface-variant/60">
                · {intl.formatMessage({ id: 'memory.used' }, { count: entry.access_count })}
              </span>
            )}
          </div>
          <p className="text-body-md text-on-surface whitespace-pre-wrap break-words mb-md">
            {entry.content}
          </p>
          {entry.tags.length > 0 && (
            <div className="flex flex-wrap gap-xs mt-sm">
              {entry.tags.map((tag) => (
                <span
                  key={tag}
                  className="text-label-xs px-sm py-[2px] rounded bg-primary-container/30 text-on-primary-container"
                >
                  #{tag}
                </span>
              ))}
            </div>
          )}
        </div>
        <div className="flex gap-xs">
          <button
            onClick={onEdit}
            className="p-xs rounded-lg hover:bg-surface-container-high cursor-pointer"
            aria-label={t('memory.action.edit')}
          >
            <span className="material-symbols-outlined text-[18px] text-on-surface-variant">edit</span>
          </button>
          <button
            onClick={onDelete}
            className="p-xs rounded-lg hover:bg-error/10 cursor-pointer"
            aria-label={t('memory.action.delete')}
          >
            <span className="material-symbols-outlined text-[18px] text-error/70">delete</span>
          </button>
        </div>
      </div>
    </div>
  )
}

function MemoryEditor({
  initial,
  onCancel,
  onSave,
}: {
  initial: MemoryEntry | null
  onCancel: () => void
  onSave: (input: {
    id?: string
    project: string
    category: MemoryCategory
    content: string
    tags: string[]
    confidence: number
  }) => Promise<void>
}) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  const [project, setProject] = useState(initial?.project ?? '.')
  const [category, setCategory] = useState<MemoryCategory>(initial?.category ?? 'context')
  const [content, setContent] = useState(initial?.content ?? '')
  const [tagsInput, setTagsInput] = useState((initial?.tags ?? []).join(', '))
  const [confidence, setConfidence] = useState(initial?.confidence ?? 1.0)
  const [saving, setSaving] = useState(false)

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!content.trim() || !project.trim()) return
    setSaving(true)
    try {
      await onSave({
        id: initial?.id,
        project: project.trim(),
        category,
        content: content.trim(),
        tags: tagsInput
          .split(',')
          .map((t) => t.trim())
          .filter(Boolean),
        confidence,
      })
    } finally {
      setSaving(false)
    }
  }

  return (
    <div
      className="fixed inset-0 z-50 bg-black/40 flex items-center justify-center p-md"
      onClick={onCancel}
    >
      <form
        onClick={(e) => e.stopPropagation()}
        onSubmit={handleSubmit}
        className="w-full max-w-2xl bg-surface rounded-2xl border border-outline-variant shadow-2xl overflow-hidden"
      >
        <header className="flex items-center justify-between px-lg py-md border-b border-outline-variant/30">
          <h2 className="text-label-lg font-bold text-on-surface">
            {initial ? t('memory.editor.edit') : t('memory.editor.create')}
          </h2>
          <button
            type="button"
            onClick={onCancel}
            className="p-xs rounded hover:bg-surface-container-high cursor-pointer"
            aria-label={t('memory.action.close')}
          >
            <span className="material-symbols-outlined text-[20px] text-on-surface-variant">close</span>
          </button>
        </header>

        <div className="p-lg space-y-md max-h-[60vh] overflow-y-auto">
          <div className="grid grid-cols-2 gap-md">
            <Field label={t('memory.editor.project')}>
              <input
                value={project}
                onChange={(e) => setProject(e.target.value)}
                required
                className="w-full px-md py-sm rounded-lg bg-surface-container-low border border-outline-variant text-label-md"
                placeholder="my-project"
              />
            </Field>
            <Field label={t('memory.editor.category')}>
              <select
                value={category}
                onChange={(e) => setCategory(e.target.value as MemoryCategory)}
                className="w-full px-md py-sm rounded-lg bg-surface-container-low border border-outline-variant text-label-md"
              >
                {(['preference', 'pattern', 'decision', 'error', 'context'] as MemoryCategory[]).map((c) => (
                  <option key={c} value={c}>
                    {t(`memory.category.${c}`)}
                  </option>
                ))}
              </select>
            </Field>
          </div>

          <Field label={t('memory.editor.content')}>
            <textarea
              value={content}
              onChange={(e) => setContent(e.target.value)}
              required
              rows={5}
              className="w-full px-md py-sm rounded-lg bg-surface-container-low border border-outline-variant text-body-md font-mono"
              placeholder={t('memory.editor.contentPlaceholder')}
            />
          </Field>

          <div className="grid grid-cols-2 gap-md">
            <Field label={t('memory.editor.tags')}>
              <input
                value={tagsInput}
                onChange={(e) => setTagsInput(e.target.value)}
                className="w-full px-md py-sm rounded-lg bg-surface-container-low border border-outline-variant text-label-md"
                placeholder="database, react, frontend"
              />
            </Field>
            {!initial && (
              <Field label={`${t('memory.editor.confidence')} (${confidence.toFixed(2)})`}>
                <input
                  type="range"
                  min="0"
                  max="1"
                  step="0.05"
                  value={confidence}
                  onChange={(e) => setConfidence(Number(e.target.value))}
                  className="w-full"
                />
              </Field>
            )}
          </div>
        </div>

        <footer className="flex justify-end gap-sm px-lg py-md border-t border-outline-variant/30 bg-surface-container-lowest">
          <button
            type="button"
            onClick={onCancel}
            className="px-md py-sm rounded-lg bg-surface-container-high text-on-surface text-label-md font-bold"
          >
            {t('memory.editor.cancel')}
          </button>
          <button
            type="submit"
            disabled={saving || !content.trim() || !project.trim()}
            className="px-md py-sm rounded-lg bg-primary text-on-primary text-label-md font-bold disabled:opacity-50"
          >
            {saving ? t('memory.editor.saving') : t('memory.editor.save')}
          </button>
        </footer>
      </form>
    </div>
  )
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="block">
      <span className="block text-label-sm text-on-surface-variant mb-xs">{label}</span>
      {children}
    </label>
  )
}
