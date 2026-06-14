import { useState, useMemo } from 'react'
import { Link } from 'react-router-dom'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import EmptyState from '@/components/ui/empty-state'
import * as api from '@/lib/tauri-api'
import type { TaskItem } from '@/types'

export type ColumnId = 'todo' | 'pending' | 'doing' | 'done' | 'deprecated'

const COLUMN_STATUSES: Record<ColumnId, string[]> = {
  todo: ['pending', 'todo'],
  pending: ['review', 'blocked'],
  doing: ['in_progress', 'running'],
  done: ['completed'],
  deprecated: ['deprecated'],
}

export function bucketFor(status: string): ColumnId {
  for (const [col, statuses] of Object.entries(COLUMN_STATUSES)) {
    if (statuses.includes(status)) return col as ColumnId
  }
  return 'todo'
}

interface Props {
  tasks: TaskItem[]
  refreshTasks: () => Promise<void> | void
}

export default function OPCKanbanBoard({ tasks, refreshTasks }: Props) {
  const [quickTask, setQuickTask] = useState('')
  const [overrides, setOverrides] = useState<Record<string, string>>({})

  const effectiveTasks: TaskItem[] = useMemo(
    () => tasks.map(t => (overrides[t.id] ? { ...t, status: overrides[t.id] } : t)),
    [tasks, overrides],
  )

  const todoTasks = effectiveTasks.filter(t => bucketFor(t.status) === 'todo')
  const pendingTasks = effectiveTasks.filter(t => bucketFor(t.status) === 'pending')
  const inProgressTasks = effectiveTasks.filter(t => bucketFor(t.status) === 'doing')
  const doneTasks = effectiveTasks.filter(t => bucketFor(t.status) === 'done')
  const deprecatedTasks = effectiveTasks.filter(t => bucketFor(t.status) === 'deprecated')

  const handleQuickTask = async () => {
    const trimmed = quickTask.trim()
    if (!trimmed) return
    try {
      await api.startBackgroundTask(trimmed)
      setQuickTask('')
      toast.success('Task created')
    } catch (e) {
      console.warn('Failed to start quick task:', e)
      toast.error('Failed to create task')
    }
  }

  // Optimistic local override — backend update_task_status is not wired yet
  const handleDrop = (taskId: string, target: ColumnId) => {
    const task = effectiveTasks.find(t => t.id === taskId)
    if (!task) return
    const current = bucketFor(task.status)
    if (current === target) return
    const newStatus = COLUMN_STATUSES[target][0]
    setOverrides(prev => ({ ...prev, [taskId]: newStatus }))
    toast.success(`Moved to ${target}`, { description: 'Local override — backend persistence pending' })
  }

  return (
    <div className="flex-1 w-full flex flex-col min-w-0">
      <div className="flex justify-between items-center mb-4">
        <h3 className="font-label-md text-[14px] font-bold text-on-surface-variant uppercase tracking-widest">KANBAN</h3>
        <div className="flex items-center gap-xs">
          <div className="relative">
            <Input
              type="text"
              placeholder="Quick inject task..."
              value={quickTask}
              onChange={e => setQuickTask(e.target.value)}
              className="bg-surface-container-low border-none rounded-lg py-1.5 pl-3 pr-8 w-[200px] text-[13px] font-body-md focus:ring-2 focus:ring-primary/20 transition-all outline-none"
              aria-label="Quick inject task"
            />
            <Button
              aria-label="Create task"
              className="absolute right-1 top-1/2 -translate-y-1/2 w-6 h-6 bg-primary text-on-primary rounded-[4px] flex items-center justify-center hover:bg-primary/90 transition-colors"
              onClick={handleQuickTask}
            >
              <span className="material-symbols-outlined text-[16px]">add</span>
            </Button>
          </div>
        </div>
      </div>

      <div className="flex gap-4 overflow-x-auto pb-4 custom-scrollbar items-start min-h-[600px]">
        <KanbanColumn title="To Do" color="bg-secondary" count={todoTasks.length} onDrop={id => handleDrop(id, 'todo')}>
          {todoTasks.map(task => <KanbanCard key={task.id} task={task} draggable />)}
        </KanbanColumn>

        <KanbanColumn title="Pending" color="bg-secondary" count={pendingTasks.length} onDrop={id => handleDrop(id, 'pending')}>
          {pendingTasks.map(task => <PendingCard key={task.id} task={task} />)}
        </KanbanColumn>

        <KanbanColumn title="Doing" color="bg-primary" count={inProgressTasks.length} onDrop={id => handleDrop(id, 'doing')}>
          {inProgressTasks.map(task => <DoingCard key={task.id} task={task} />)}
        </KanbanColumn>

        <KanbanColumn title="Done" color="bg-tertiary" count={doneTasks.length} onDrop={id => handleDrop(id, 'done')}>
          {doneTasks.map(task => <DoneCard key={task.id} task={task} />)}
        </KanbanColumn>

        <KanbanColumn title="Deprecated" color="bg-outline-variant" count={deprecatedTasks.length} onDrop={id => handleDrop(id, 'deprecated')}>
          {deprecatedTasks.length === 0 ? (
            <div className="flex items-center justify-center p-xl mt-xl">
              <EmptyState icon="archive" title="No deprecated tasks." description="Completed or cancelled tasks will appear here." />
            </div>
          ) : (
            deprecatedTasks.map(task => <KanbanCard key={task.id} task={task} draggable />)
          )}
        </KanbanColumn>
      </div>

      {Object.keys(overrides).length > 0 ? (
        <div className="mt-sm flex justify-end">
          <button
            className="text-label-sm text-on-surface-variant hover:text-primary cursor-pointer underline"
            onClick={() => { setOverrides({}); void refreshTasks() }}
          >
            Reset local overrides
          </button>
        </div>
      ) : null}
    </div>
  )
}

function KanbanColumn({
  title,
  color,
  count,
  children,
  onDrop,
}: {
  title: string
  color: string
  count: number
  children: React.ReactNode
  onDrop?: (taskId: string) => void
}) {
  const [isOver, setIsOver] = useState(false)
  return (
    <div
      className={`w-[300px] shrink-0 rounded-xl p-xs border transition-colors ${isOver ? 'bg-surface-container-high/40 border-primary/40' : 'bg-surface-container-lowest/50 border-transparent hover:bg-surface-container-low/30'}`}
      onDragOver={e => { e.preventDefault(); setIsOver(true) }}
      onDragLeave={() => setIsOver(false)}
      onDrop={e => {
        e.preventDefault()
        setIsOver(false)
        const taskId = e.dataTransfer.getData('text/plain')
        if (taskId && onDrop) onDrop(taskId)
      }}
    >
      <div className="flex justify-between items-center px-2 py-3 mb-1">
        <div className="flex items-center gap-2">
          <span className={`w-2 h-2 rounded-full ${color}`} />
          <span className="font-label-md text-[14px] font-bold">{title}</span>
        </div>
        <span className="font-label-sm text-[11px] text-on-surface-variant">{count}</span>
      </div>
      {children}
      {count === 0 && title !== 'Deprecated' ? (
        <div className="flex items-center justify-center p-xl mt-xl">
          <p className="font-label-sm text-[12px] text-on-surface-variant italic opacity-60">Empty</p>
        </div>
      ) : null}
    </div>
  )
}

function KanbanCard({ task, draggable }: { task: { id: string; title: string; description?: string; assignee?: string; priority?: string }; draggable?: boolean }) {
  return (
    <div
      draggable={draggable}
      onDragStart={draggable ? (e => e.dataTransfer.setData('text/plain', task.id)) : undefined}
      className="bg-surface-container-lowest rounded-xl p-md border border-outline-variant/30 shadow-sm mb-3 cursor-pointer hover:border-primary/50 hover:shadow-md transition-all group/card focus-visible:ring-2 focus-visible:ring-primary/30 focus-visible:outline-none"
      tabIndex={0}
      role="button"
      onKeyDown={e => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); window.location.hash = `/opc/task/${task.id}` } }}
    >
      <div className="flex justify-between items-start mb-2">
        <span className="font-label-sm text-[10px] font-bold text-on-surface-variant tracking-wider">{task.id.slice(0, 8)}</span>
        {task.priority ? (
          <span className={`text-[10px] font-bold px-2 py-0.5 rounded uppercase tracking-wider ${
            task.priority === 'high' ? 'bg-error/10 text-error' : task.priority === 'medium' ? 'bg-secondary/10 text-secondary' : 'bg-surface-container text-on-surface-variant'
          }`}>{task.priority}</span>
        ) : null}
      </div>
      <h4 className="font-label-md text-[15px] font-bold mb-3 leading-tight group-hover/card:text-primary transition-colors">{task.title}</h4>
      {task.description ? <p className="font-body-sm text-[12px] text-on-surface-variant mb-2 leading-snug line-clamp-2">{task.description}</p> : null}
      <div className="flex justify-between items-center">
        {task.assignee ? (
          <span className="font-label-sm text-[11px] text-on-surface-variant">Proposed by <strong className="text-on-surface">{task.assignee}</strong></span>
        ) : null}
      </div>
    </div>
  )
}

function PendingCard({ task }: { task: TaskItem }) {
  return (
    <div
      draggable
      onDragStart={e => e.dataTransfer.setData('text/plain', task.id)}
      className="bg-surface-container-lowest rounded-xl p-md border border-error/20 shadow-sm mb-3 ring-1 ring-error/5 cursor-grab active:cursor-grabbing hover:border-error/40 transition-colors relative"
    >
      <div className="absolute left-0 top-0 bottom-0 w-1 bg-error rounded-l-xl" />
      <div className="flex justify-between items-start mb-2 ml-1">
        <span className="font-label-sm text-[10px] font-bold text-on-surface-variant tracking-wider">{task.id.slice(0, 8)}</span>
        {task.priority === 'high' ? (
          <span className="bg-error/10 text-error text-[10px] font-bold px-2 py-0.5 rounded uppercase tracking-wider">Critical</span>
        ) : null}
      </div>
      <h4 className="font-label-md text-[15px] font-bold mb-2 leading-tight ml-1">{task.title}</h4>
      {task.assignee ? (
        <div className="flex justify-between items-center ml-1">
          <span className="font-label-sm text-[11px] text-on-surface-variant">Assigned to <strong className="text-on-surface">{task.assignee}</strong></span>
          <span className="font-label-sm text-[12px] text-primary font-bold">Review</span>
        </div>
      ) : null}
    </div>
  )
}

function DoingCard({ task }: { task: TaskItem }) {
  return (
    <div
      draggable
      onDragStart={e => e.dataTransfer.setData('text/plain', task.id)}
      className="bg-surface-container-lowest rounded-xl p-md border border-primary/20 shadow-sm mb-3 cursor-grab active:cursor-grabbing hover:border-primary/50 transition-colors relative"
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
            <span className="font-label-sm text-[10px] font-bold text-on-surface-variant">{task.progress != null ? `${Math.min(100, Math.max(0, Math.round(task.progress)))}%` : 'In Progress'}</span>
          </div>
        </div>
      ) : null}
    </div>
  )
}

function DoneCard({ task }: { task: TaskItem }) {
  return (
    <Link
      to="/opc/task"
      draggable
      onDragStart={e => e.dataTransfer.setData('text/plain', task.id)}
      className="block bg-surface-container-lowest rounded-xl p-3 border border-tertiary/20 shadow-sm mb-3 cursor-grab active:cursor-grabbing hover:bg-surface-bright transition-colors bg-tertiary/5"
    >
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span className="material-symbols-outlined text-[16px] text-tertiary">check_circle</span>
          <span className="font-label-md text-[13px] text-on-surface">{task.title}</span>
        </div>
        <span className="font-label-sm text-[10px] text-on-surface-variant">Done</span>
      </div>
    </Link>
  )
}
