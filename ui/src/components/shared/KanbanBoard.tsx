// Generic kanban primitive used by Mission Control (read-only) and OPC (drag-and-drop).
//
// Why this exists:
//   - MissionControl.tsx and OPCKanbanBoard.tsx previously each defined their own
//     column taxonomy with different names and status mappings. A card "In Progress"
//     in MC was "Doing" in OPC; "Failed" existed in one and not the other; and
//     "Pending" was ambiguous (review or blocked?). Tasks moving between pages
//     changed meaning. This component fixes that: there is one column taxonomy,
//     defined in lib/task-status.ts, and both surfaces render through this file.
//
// Mode switch via the `mode` prop:
//   - 'observe'  → click cards to select (Mission Control). No DnD.
//   - 'interact' → drag cards between columns (OPC). Optimistic local override.
//
// Card rendering is pluggable so each page can keep its visual identity:
//   - Default renderer is provided (shows title, assignee, team, due, priority).
//   - OPC passes a custom renderer to keep its column-themed accents.

import { useMemo, useState, type ReactNode } from 'react'
import { useIntl } from 'react-intl'
import type { TaskItem } from '@/types'
import {
  STATUS_FAMILY,
  DEFAULT_COLUMN_ORDER,
  groupTasksByFamily,
  classifyStatus,
  canonicalStatusFor,
  PRIORITY_RANK,
  type TaskStatusFamily,
} from '@/lib/task-status'

export interface KanbanBoardProps {
  tasks: TaskItem[]
  /** 'observe' = read-only grid (Mission Control); 'interact' = DnD surface (OPC). */
  mode?: 'observe' | 'interact'
  /** Restrict which columns show and in what order. Defaults to all five. */
  columns?: TaskStatusFamily[]
  /** Click handler in observe mode. */
  onSelectTask?: (id: string) => void
  /** Drop handler in interact mode. Parent should persist + call refreshTasks. */
  onMoveTask?: (id: string, target: TaskStatusFamily) => void
  /** Custom card renderer. Falls back to DefaultCard. */
  renderCard?: (task: TaskItem, family: TaskStatusFamily) => ReactNode
  /** Optional empty-state message per column. */
  emptyLabel?: (family: TaskStatusFamily) => string | undefined
  /** Optional header right-side content (e.g. totals chips). */
  headerExtra?: ReactNode
  /** Optional title shown above the board (small, uppercase). */
  boardTitle?: string
  /** Optional right-aligned toolbar (e.g. quick-inject input). */
  toolbar?: ReactNode
}

const PRIORITY_RING: Record<string, string> = {
  critical: 'ring-error/40',
  high: 'ring-warning/40',
  normal: 'ring-outline-variant/30',
  low: 'ring-outline-variant/20',
}

export function KanbanBoard({
  tasks,
  mode = 'observe',
  columns = DEFAULT_COLUMN_ORDER,
  onSelectTask,
  onMoveTask,
  renderCard,
  emptyLabel,
  headerExtra,
  boardTitle,
  toolbar,
}: KanbanBoardProps) {
  const intl = useIntl()
  const grouped = useMemo(() => groupTasksByFamily(tasks), [tasks])
  const totals = useMemo(() => {
    const t: Record<TaskStatusFamily, number> = { queued: 0, active: 0, blocked: 0, done: 0, failed: 0 }
    for (const key of columns) t[key] = grouped[key].length
    return t
  }, [grouped, columns])

  return (
    <div className="flex-1 w-full flex flex-col min-w-0">
      {(boardTitle || toolbar) && (
        <div className="flex justify-between items-center mb-4">
          {boardTitle ? (
            <h3 className="font-label-md text-[14px] font-bold text-on-surface-variant uppercase tracking-widest">
              {boardTitle}
            </h3>
          ) : <span />}
          {toolbar}
          {headerExtra}
        </div>
      )}

      <div
        className={mode === 'observe'
          ? 'flex-1 overflow-x-auto overflow-y-hidden'
          : 'flex gap-4 overflow-x-auto pb-4 custom-scrollbar items-start min-h-[600px]'}
        role="grid"
        aria-label={intl.formatMessage({ id: 'shared.kanban.board.aria' })}
      >
        <div className={mode === 'observe'
          ? 'h-full flex gap-md p-lg min-w-max'
          : 'contents'}>
          {columns.map(col => {
            const meta = STATUS_FAMILY[col]
            const items = grouped[col]
            return (
              <KanbanColumn
                key={col}
                family={col}
                title={intl.formatMessage({ id: meta.titleKey })}
                icon={meta.icon}
                dotClass={meta.dotClass}
                bgClass={meta.bgClass}
                count={totals[col]}
                observe={mode === 'observe'}
                emptyLabel={emptyLabel?.(col)}
                onDrop={mode === 'interact' ? id => onMoveTask?.(id, col) : undefined}
              >
                {items.length === 0 ? null : items.map(task => {
                  const fam = classifyStatus(task.status)
                  return renderCard
                    ? renderCard(task, fam)
                    : <DefaultCard key={task.id} task={task} onClick={onSelectTask} observe={mode === 'observe'} />
                })}
              </KanbanColumn>
            )
          })}
        </div>
      </div>
    </div>
  )
}

interface KanbanColumnProps {
  family: TaskStatusFamily
  title: string
  icon: string
  dotClass: string
  bgClass: string
  count: number
  observe: boolean
  emptyLabel?: string
  onDrop?: (taskId: string) => void
  children: ReactNode
}

function KanbanColumn({ title, icon, dotClass, bgClass, count, observe, emptyLabel, onDrop, children }: KanbanColumnProps) {
  const intl = useIntl()
  const [isOver, setIsOver] = useState(false)
  return (
    <section
      role="row"
      aria-label={title}
      onDragOver={onDrop ? e => { e.preventDefault(); setIsOver(true) } : undefined}
      onDragLeave={onDrop ? () => setIsOver(false) : undefined}
      onDrop={onDrop ? e => {
        e.preventDefault()
        setIsOver(false)
        const taskId = e.dataTransfer.getData('text/plain')
        if (taskId) onDrop(taskId)
      } : undefined}
      className={observe
        ? `flex flex-col w-[300px] rounded-2xl border border-outline-variant/20 ${bgClass}`
        : `w-[300px] shrink-0 rounded-xl p-xs border transition-colors ${isOver ? 'bg-surface-container-high/40 border-primary/40' : `bg-surface-container-lowest/50 border-transparent hover:bg-surface-container-low/30`}`}
    >
      <header className={observe
        ? 'flex items-center justify-between px-md py-sm border-b border-outline-variant/20'
        : 'flex justify-between items-center px-2 py-3 mb-1'}>
        <div className="flex items-center gap-2">
          <span className={`w-2 h-2 rounded-full ${dotClass}`} />
          {observe ? (
            <span className="font-label-md text-label-md text-on-surface font-bold uppercase tracking-wider flex items-center gap-xs">
              <span className="material-symbols-outlined text-[16px]">{icon}</span>
              {title}
            </span>
          ) : (
            <span className="font-label-md text-[14px] font-bold">{title}</span>
          )}
        </div>
        <span className={observe
          ? 'text-label-sm text-on-surface-variant font-mono'
          : 'font-label-sm text-[11px] text-on-surface-variant'}>
          {count}
        </span>
      </header>
      <div className={observe ? 'flex-1 overflow-y-auto p-sm space-y-sm' : undefined}>
        {count === 0 ? (
          <div className={observe
            ? 'text-center text-label-sm text-on-surface-variant py-xl opacity-60'
            : 'flex items-center justify-center p-xl mt-xl'}>
            <p className={observe ? '' : 'font-label-sm text-[12px] text-on-surface-variant italic opacity-60'}>
              {emptyLabel ?? intl.formatMessage({ id: 'shared.kanban.nothingHere' })}
            </p>
          </div>
        ) : children}
      </div>
    </section>
  )
}

export function DefaultCard({ task, onClick, observe }: { task: TaskItem; onClick?: (id: string) => void; observe?: boolean }) {
  const priority = task.priority ?? 'normal'
  const ring = PRIORITY_RING[priority] ?? PRIORITY_RING.normal
  const handleActivate = () => onClick?.(task.id)
  return (
    <button
      type="button"
      onClick={observe ? handleActivate : undefined}
      draggable={!observe}
      onDragStart={!observe ? e => e.dataTransfer.setData('text/plain', task.id) : undefined}
      className={
        'w-full text-left p-md rounded-xl bg-surface-container-lowest/90 shadow-sm hover:shadow-md hover:-translate-y-0.5 transition-all duration-200 mb-3 cursor-pointer focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/40 ring-1 ' + ring
      }
    >
      <div className="flex items-start justify-between gap-xs">
        <h4 className="font-body-md font-semibold text-on-surface line-clamp-2 flex-1">{task.title}</h4>
        {task.priority ? (
          <span className="shrink-0 text-[10px] uppercase tracking-wider font-bold text-on-surface-variant px-xs py-0.5 rounded bg-surface-container-high">
            {task.priority}
          </span>
        ) : null}
      </div>
      {task.description ? (
        <p className="text-label-sm text-on-surface-variant mt-xs line-clamp-2">{task.description}</p>
      ) : null}
      <div className="mt-sm flex items-center gap-md text-label-sm text-on-surface-variant">
        {task.assignee ? (
          <span className="inline-flex items-center gap-xs">
            <span className="material-symbols-outlined text-[14px]">smart_toy</span>
            {task.assignee}
          </span>
        ) : null}
        {task.team ? (
          <span className="inline-flex items-center gap-xs">
            <span className="material-symbols-outlined text-[14px]">groups</span>
            {task.team}
          </span>
        ) : null}
        {task.due_date ? (
          <span className="inline-flex items-center gap-xs">
            <span className="material-symbols-outlined text-[14px]">event</span>
            {new Date(task.due_date * 1000).toLocaleDateString()}
          </span>
        ) : null}
      </div>
    </button>
  )
}

export { classifyStatus, canonicalStatusFor, STATUS_FAMILY, PRIORITY_RANK }
export type { TaskStatusFamily }
