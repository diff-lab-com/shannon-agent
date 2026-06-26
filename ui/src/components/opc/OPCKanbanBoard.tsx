// OPC Kanban Board — agent-orchestration workspace surface.
//
// This component was refactored to consume the shared KanbanBoard primitive
// (mode='interact') and the unified status taxonomy in lib/task-status.ts.
//
// What changed:
//   - Column taxonomy is identical to Mission Control now (queued, active,
//     blocked, done, failed). Previously OPC used todo/pending/doing/done/
//     deprecated with different status mappings — moving a card between
//     Mission Control and OPC changed its meaning.
//   - The optimistic local-override mechanism and variant card visuals are
//     preserved (progress bar on active, red accent on blocked, etc.) by
//     passing a custom renderCard to the shared primitive.
//   - bucketFor() is kept as a thin wrapper over classifyStatus() so existing
//     callers (and tests) keep working. It now returns the unified family.

import { useCallback, useMemo, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import * as api from '@/lib/tauri-api'
import type { TaskItem } from '@/types'
import { KanbanBoard } from '@/components/shared/KanbanBoard'
import {
  classifyStatus,
  canonicalStatusFor,
  STATUS_FAMILY,
  normalizePriority,
  type TaskStatusFamily,
} from '@/lib/task-status'

export type ColumnId = TaskStatusFamily

/** Legacy alias: maps any raw status to a unified column family. */
export function bucketFor(status: string): ColumnId {
  return classifyStatus(status)
}

interface Props {
  tasks: TaskItem[]
  refreshTasks: () => Promise<void> | void
}

export default function OPCKanbanBoard({ tasks, refreshTasks }: Props) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const navigate = useNavigate()
  const [quickTask, setQuickTask] = useState('')
  const [overrides, setOverrides] = useState<Record<string, string>>({})

  const openTask = useCallback((id: string) => {
    navigate(`/opc/task/${id}`)
  }, [navigate])

  const effectiveTasks: TaskItem[] = useMemo(
    () => tasks.map(t => (overrides[t.id] ? { ...t, status: overrides[t.id] } : t)),
    [tasks, overrides],
  )

  const handleQuickTask = async () => {
    const trimmed = quickTask.trim()
    if (!trimmed) return
    try {
      await api.startBackgroundTask(trimmed)
      setQuickTask('')
      toast.success(t('opc.kanban.taskCreated'))
    } catch (e) {
      console.warn('Failed to start quick task:', e)
      toast.error(t('opc.kanban.createFailed'))
    }
  }

  // Optimistic local override + backend persist via update_task(status).
  // The override is applied immediately so the card jumps without waiting
  // for the round-trip; refreshTasks() reconciles after the write lands.
  const handleMoveTask = async (taskId: string, target: TaskStatusFamily) => {
    const task = effectiveTasks.find(t => t.id === taskId)
    if (!task) return
    const current = classifyStatus(task.status)
    if (current === target) return
    const newStatus = canonicalStatusFor(target)
    setOverrides(prev => ({ ...prev, [taskId]: newStatus }))
    toast.success(intl.formatMessage({ id: 'opc.kanban.movedTo' }, { column: intl.formatMessage({ id: STATUS_FAMILY[target].titleKey }) }))
    try {
      await api.updateTask({ id: taskId, status: newStatus })
      setOverrides(prev => {
        const next = { ...prev }
        delete next[taskId]
        return next
      })
      await refreshTasks()
    } catch (e) {
      console.warn('updateTask failed, rolling back override:', e)
      setOverrides(prev => {
        const next = { ...prev }
        delete next[taskId]
        return next
      })
      toast.error(t('opc.kanban.moveFailed'))
      await refreshTasks()
    }
  }

  const toolbar = (
    <div className="relative">
      <Input
        type="text"
        placeholder={t('opc.kanban.addTaskPlaceholder')}
        value={quickTask}
        onChange={e => setQuickTask(e.target.value)}
        className="bg-surface-container-low border-none rounded-lg py-1.5 pl-3 pr-8 w-[200px] text-[13px] font-body-md focus:ring-2 focus:ring-primary/20 transition-all outline-none"
        aria-label={t('opc.kanban.addTaskAria')}
      />
      <Button
        aria-label={t('opc.kanban.createTaskAria')}
        className="absolute right-1 top-1/2 -translate-y-1/2 w-6 h-6 bg-primary text-on-primary rounded-[4px] flex items-center justify-center hover:bg-primary/90 transition-colors"
        onClick={handleQuickTask}
      >
        <span className="material-symbols-outlined text-[16px]">add</span>
      </Button>
    </div>
  )

  return (
    <div className="flex-1 w-full flex flex-col min-w-0">
      <KanbanBoard
        tasks={effectiveTasks}
        mode="interact"
        boardTitle={t('opc.kanban.boardTitle')}
        toolbar={toolbar}
        onMoveTask={handleMoveTask}
        renderCard={(task, family) => (
          <VariantCard key={task.id} task={task} family={family} intl={intl} openTask={openTask} />
        )}
        emptyLabel={family => family === 'failed'
          ? t('opc.kanban.noDeprecated')
          : undefined}
      />

      {Object.keys(overrides).length > 0 ? (
        <div className="mt-sm flex justify-end">
          <button
            className="text-label-sm text-on-surface-variant hover:text-primary cursor-pointer underline"
            onClick={() => { setOverrides({}); void refreshTasks() }}
          >
            {t('opc.kanban.resetOverrides')}
          </button>
        </div>
      ) : null}
    </div>
  )
}

/**
 * Dispatches to a variant card based on column family, preserving the visual
 * language the OPC board already had: red accent on blocked, progress bar on
 * active, check row on done, etc. Default falls through to a draggable card
 * that matches the old "todo" / "deprecated" look. All variants are clickable
 * + keyboard-accessible so users can open task detail from any column.
 */
function VariantCard({ task, family, intl, openTask }: { task: TaskItem; family: TaskStatusFamily; intl: ReturnType<typeof useIntl>; openTask: (id: string) => void }) {
  switch (family) {
    case 'blocked': return <BlockedCard task={task} intl={intl} openTask={openTask} />
    case 'active':  return <ActiveCard task={task} intl={intl} openTask={openTask} />
    case 'done':    return <DoneCard task={task} intl={intl} openTask={openTask} />
    case 'failed':  return <FailedCard task={task} intl={intl} openTask={openTask} />
    default:        return <DefaultDraggableCard task={task} intl={intl} openTask={openTask} />
  }
}

function DefaultDraggableCard({ task, intl, openTask }: { task: TaskItem; intl: ReturnType<typeof useIntl>; openTask: (id: string) => void }) {
  return (
    <div
      draggable
      onDragStart={e => e.dataTransfer.setData('text/plain', task.id)}
      className="bg-surface-container-lowest rounded-xl p-md border border-outline-variant/30 shadow-sm mb-3 cursor-pointer hover:border-primary/50 hover:shadow-md transition-all group/card focus-visible:ring-2 focus-visible:ring-primary/30 focus-visible:outline-none"
      tabIndex={0}
      role="button"
      aria-label={task.title}
      onClick={() => openTask(task.id)}
      onKeyDown={e => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); openTask(task.id) } }}
    >
      <div className="flex justify-between items-start mb-2">
        <span className="font-label-sm text-[10px] font-bold text-on-surface-variant tracking-wider">{task.id.slice(0, 8)}</span>
        {task.priority ? (
          <span className={`text-[10px] font-bold px-2 py-0.5 rounded uppercase tracking-wider ${
            normalizePriority(task.priority) === 'critical' ? 'bg-error/20 text-error' :
            normalizePriority(task.priority) === 'high' ? 'bg-error/10 text-error' :
            normalizePriority(task.priority) === 'medium' ? 'bg-secondary/10 text-secondary' :
            normalizePriority(task.priority) === 'low' ? 'bg-surface-container text-on-surface-variant' :
            'bg-surface-container text-on-surface-variant'
          }`}>{task.priority}</span>
        ) : null}
      </div>
      <h4 className="font-label-md text-[15px] font-bold mb-3 leading-tight group-hover/card:text-primary transition-colors">{task.title}</h4>
      {task.description ? <p className="font-body-sm text-[12px] text-on-surface-variant mb-2 leading-snug line-clamp-2">{task.description}</p> : null}
      <div className="flex justify-between items-center">
        {task.assignee ? (
          <span className="font-label-sm text-[11px] text-on-surface-variant">{intl.formatMessage({ id: 'opc.kanban.proposedBy' }, { name: task.assignee })}</span>
        ) : null}
      </div>
    </div>
  )
}

function BlockedCard({ task, intl, openTask }: { task: TaskItem; intl: ReturnType<typeof useIntl>; openTask: (id: string) => void }) {
  return (
    <div
      draggable
      onDragStart={e => e.dataTransfer.setData('text/plain', task.id)}
      onClick={() => openTask(task.id)}
      onKeyDown={e => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); openTask(task.id) } }}
      className="bg-surface-container-lowest rounded-xl p-md border border-error/20 shadow-sm mb-3 ring-1 ring-error/5 cursor-grab active:cursor-grabbing hover:border-error/40 transition-colors relative focus-visible:ring-2 focus-visible:ring-primary/30 focus-visible:outline-none"
      tabIndex={0}
      role="button"
      aria-label={intl.formatMessage({ id: 'opc.kanban.openTaskAria' }, { title: task.title })}
    >
      <div className="absolute left-0 top-0 bottom-0 w-1 bg-error rounded-l-xl" />
      <div className="flex justify-between items-start mb-2 ml-1">
        <span className="font-label-sm text-[10px] font-bold text-on-surface-variant tracking-wider">{task.id.slice(0, 8)}</span>
        {(normalizePriority(task.priority) === 'high' || normalizePriority(task.priority) === 'critical') ? (
          <span className="bg-error/10 text-error text-[10px] font-bold px-2 py-0.5 rounded uppercase tracking-wider">{intl.formatMessage({ id: 'opc.kanban.critical' })}</span>
        ) : null}
      </div>
      <h4 className="font-label-md text-[15px] font-bold mb-2 leading-tight ml-1">{task.title}</h4>
      {task.assignee ? (
        <div className="flex justify-between items-center ml-1">
          <span className="font-label-sm text-[11px] text-on-surface-variant">{intl.formatMessage({ id: 'opc.kanban.assignedTo' }, { name: task.assignee })}</span>
          <span className="font-label-sm text-[12px] text-primary font-bold">{intl.formatMessage({ id: 'opc.kanban.review' })}</span>
        </div>
      ) : null}
    </div>
  )
}

function ActiveCard({ task, intl, openTask }: { task: TaskItem; intl: ReturnType<typeof useIntl>; openTask: (id: string) => void }) {
  return (
    <div
      draggable
      onDragStart={e => e.dataTransfer.setData('text/plain', task.id)}
      onClick={() => openTask(task.id)}
      onKeyDown={e => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); openTask(task.id) } }}
      className="bg-surface-container-lowest rounded-xl p-md border border-primary/20 shadow-sm mb-3 cursor-grab active:cursor-grabbing hover:border-primary/50 transition-colors relative focus-visible:ring-2 focus-visible:ring-primary/30 focus-visible:outline-none"
      tabIndex={0}
      role="button"
      aria-label={intl.formatMessage({ id: 'opc.kanban.openTaskAria' }, { title: task.title })}
    >
      <div className="absolute left-0 top-0 bottom-0 w-1 bg-primary rounded-l-xl" />
      <div className="flex justify-between items-center mb-2 ml-1">
        <span className="font-label-sm text-[10px] font-bold text-primary tracking-wider">{task.id.slice(0, 8)}</span>
        <span className="material-symbols-outlined text-[16px] text-primary">autorenew</span>
      </div>
      <h4 className="font-label-md text-[15px] font-bold mb-4 leading-tight ml-1">{task.title}</h4>
      {task.assignee ? (
        <div className="ml-1 mb-2">
          <div className="h-1.5 w-full bg-surface-container rounded-full overflow-hidden mb-1">
            {task.progress != null ? (
              <div className="h-full bg-primary rounded-full transition-all duration-500" style={{ width: `${Math.min(100, Math.max(0, task.progress))}%` }} />
            ) : (
              <div className="h-full w-1/3 bg-primary/60 rounded-full animate-pulse" />
            )}
          </div>
          <div className="flex justify-between items-center">
            <span className="font-label-sm text-[10px] text-on-surface-variant">{task.assignee}</span>
            <span className="font-label-sm text-[10px] font-bold text-on-surface-variant">{task.progress != null ? `${Math.min(100, Math.max(0, Math.round(task.progress)))}%` : intl.formatMessage({ id: 'opc.kanban.inProgress' })}</span>
          </div>
        </div>
      ) : null}
    </div>
  )
}

function DoneCard({ task, intl, openTask }: { task: TaskItem; intl: ReturnType<typeof useIntl>; openTask: (id: string) => void }) {
  return (
    <div
      draggable
      onDragStart={e => e.dataTransfer.setData('text/plain', task.id)}
      onClick={() => openTask(task.id)}
      onKeyDown={e => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); openTask(task.id) } }}
      className="bg-surface-container-lowest rounded-xl p-3 border border-tertiary/20 shadow-sm mb-3 cursor-grab active:cursor-grabbing hover:bg-surface-bright transition-colors bg-tertiary/5 focus-visible:ring-2 focus-visible:ring-primary/30 focus-visible:outline-none"
      tabIndex={0}
      role="button"
      aria-label={intl.formatMessage({ id: 'opc.kanban.openTaskAria' }, { title: task.title })}
    >
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span className="material-symbols-outlined text-[16px] text-tertiary">check_circle</span>
          <span className="font-label-md text-[13px] text-on-surface">{task.title}</span>
        </div>
        <span className="font-label-sm text-[10px] text-on-surface-variant">{intl.formatMessage({ id: 'opc.kanban.done' })}</span>
      </div>
    </div>
  )
}

function FailedCard({ task, intl, openTask }: { task: TaskItem; intl: ReturnType<typeof useIntl>; openTask: (id: string) => void }) {
  return (
    <div
      draggable
      onDragStart={e => e.dataTransfer.setData('text/plain', task.id)}
      onClick={() => openTask(task.id)}
      onKeyDown={e => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); openTask(task.id) } }}
      className="bg-surface-container-lowest/50 rounded-xl p-md border border-outline-variant/30 shadow-sm mb-3 cursor-grab active:cursor-grabbing hover:bg-surface-container-lowest transition-colors opacity-70 focus-visible:ring-2 focus-visible:ring-primary/30 focus-visible:outline-none"
      tabIndex={0}
      role="button"
      aria-label={intl.formatMessage({ id: 'opc.kanban.openTaskAria' }, { title: task.title })}
    >
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span className="material-symbols-outlined text-[16px] text-on-surface-variant">archive</span>
          <span className="font-label-md text-[13px] text-on-surface-variant line-through">{task.title}</span>
        </div>
        <span className="font-label-sm text-[10px] text-on-surface-variant uppercase tracking-wider">{intl.formatMessage({ id: 'opc.kanban.archived' })}</span>
      </div>
    </div>
  )
}
