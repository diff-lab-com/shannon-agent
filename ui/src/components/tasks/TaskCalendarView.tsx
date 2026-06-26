// Full-month calendar view for the Tasks page.
//
// MD3 tokens. Replaces the list view when calendarView is true. Renders a
// 7-column grid, a per-day task list when a day is selected, and a row of
// bottom widgets (EfficiencyCard, AgentAllocation, Active Now).
//
// Scheduled-routine integration: routines with `next_fire_at` in the current
// view month are rendered as colored bars on their fire day. Falls back to
// the original heuristic (highlight days with running/completed tasks) when no
// scheduled data is supplied.

import { useIntl } from 'react-intl'
import type { TaskItem, AgentInfo, ScheduledRoutine } from '@/types'
import { DAY_NAMES, MONTH_NAMES, statusBadge } from './shared'
import EfficiencyCard from './EfficiencyCard'
import AgentAllocation from './AgentAllocation'

interface TaskCalendarViewProps {
  viewMonth: number
  viewYear: number
  selectedDay: number | null
  filteredTasks: TaskItem[]
  allTasks: TaskItem[]
  agents: AgentInfo[]
  scheduledTasks?: ScheduledRoutine[]
  efficiencyPct: number
  onSelectDay: (day: number | null) => void
  onSelectTask: (id: string) => void
}

export default function TaskCalendarView({
  viewMonth,
  viewYear,
  selectedDay,
  filteredTasks,
  allTasks,
  agents,
  scheduledTasks = [],
  efficiencyPct,
  onSelectDay,
  onSelectTask,
}: TaskCalendarViewProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const today = new Date()
  const startDay = (new Date(viewYear, viewMonth, 1).getDay() + 6) % 7 // Monday-based
  const daysInMonth = new Date(viewYear, viewMonth + 1, 0).getDate()

  // Map day-of-month -> routines firing that day (from next_fire_at).
  const firesByDay = new Map<number, ScheduledRoutine[]>()
  for (const r of scheduledTasks) {
    if (r.next_fire_at == null) continue
    const d = new Date(r.next_fire_at * 1000)
    if (d.getMonth() === viewMonth && d.getFullYear() === viewYear) {
      const arr = firesByDay.get(d.getDate()) ?? []
      arr.push(r)
      firesByDay.set(d.getDate(), arr)
    }
  }

  return (
    <div className="space-y-lg">
      {/* Full-Width Calendar Grid */}
      <div className="bg-surface-container-lowest border border-outline-variant/30 rounded-2xl p-lg shadow-sm">
        <div className="grid grid-cols-7 text-center mb-sm">
          {DAY_NAMES.map(d => <span key={d} className="text-[11px] font-bold text-outline uppercase py-sm">{d}</span>)}
        </div>
        <div className="grid grid-cols-7 gap-1">
          {Array.from({ length: startDay }, (_, i) => (
            <div key={`prev-${i}`} className="min-h-[80px] p-xs rounded-lg" />
          ))}
          {Array.from({ length: daysInMonth }, (_, i) => {
            const day = i + 1
            const isToday = viewMonth === today.getMonth() && viewYear === today.getFullYear() && day === today.getDate()
            const dayFires = firesByDay.get(day) ?? []
            const isSelected = selectedDay === day
            return (
              <div
                key={day}
                title={dayFires.length > 0 ? intl.formatMessage({ id: 'tasks.taskCalendarView.scheduledRuns' }, { count: dayFires.length }) : undefined}
                className={`min-h-[80px] p-xs rounded-lg border cursor-pointer transition-all ${
                  isSelected ? 'border-primary bg-primary/5 ring-1 ring-primary/20' :
                  isToday ? 'border-primary/30 bg-primary/5' :
                  'border-outline-variant/10 hover:bg-surface-container-low'
                }`}
                onClick={() => onSelectDay(isSelected ? null : day)}
              >
                <div className={`text-[12px] font-bold mb-xs ${isToday ? 'w-6 h-6 rounded-full bg-primary text-on-primary flex items-center justify-center' : 'text-on-surface-variant'}`}>
                  {day}
                </div>
                <div className="space-y-0.5">
                  {dayFires.slice(0, 3).map((r, ri) => (
                    <div key={`fire-${ri}`} className="h-1 rounded-full bg-secondary" title={r.name} />
                  ))}
                  {dayFires.length > 3 && (
                    <span className="text-[9px] text-on-surface-variant">+{dayFires.length - 3}</span>
                  )}
                </div>
              </div>
            )
          })}
        </div>
      </div>

      {/* Tasks for Selected Day */}
      {selectedDay !== null && (
        <div>
          <h4 className="font-label-md text-label-md text-outline uppercase tracking-[0.1em] mb-md pl-xs">
            {MONTH_NAMES[viewMonth]} {selectedDay} — {t('tasks.taskCalendarView.tasks')}
          </h4>
          <div className="space-y-md">
            {filteredTasks.length === 0 ? (
              <p className="text-body-sm text-on-surface-variant text-center py-lg">{t('tasks.taskCalendarView.noTasks')}</p>
            ) : (
              filteredTasks.slice(0, 5).map(task => {
                const badge = statusBadge(task.status)
                return (
                  <div
                    key={task.id}
                    className="glass-panel border border-outline-variant/10 rounded-xl p-md shadow-sm hover:shadow-md transition-all group bg-surface-container-lowest/80 cursor-pointer"
                    onClick={() => onSelectTask(task.id)}
                  >
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-md">
                        <div className="w-10 h-10 rounded-xl bg-primary/10 flex items-center justify-center text-primary">
                          <span className="material-symbols-outlined icon-lg">task_alt</span>
                        </div>
                        <div>
                          <h3 className="font-body-lg font-semibold text-on-surface group-hover:text-primary transition-colors">{task.title}</h3>
                          {task.assignee ? <span className="font-label-sm text-on-surface-variant">{task.assignee}</span> : null}
                        </div>
                      </div>
                      <div title={badge.tip} className={`flex items-center gap-xs px-sm py-1 rounded-full border ${badge.bg}`}>
                        <span className={`w-2 h-2 rounded-full ${badge.dot}`} />
                        <span className="font-label-sm text-[11px] font-bold uppercase tracking-wider">{badge.label}</span>
                      </div>
                    </div>
                  </div>
                )
              })
            )}
          </div>
        </div>
      )}

      {/* Bottom Widgets in Calendar Mode */}
      <div className="grid grid-cols-1 md:grid-cols-3 gap-gutter">
        <EfficiencyCard percentage={efficiencyPct} variant="compact" />
        <AgentAllocation agents={agents} />
        <div className="bg-surface-container-lowest border border-outline-variant/30 rounded-2xl p-lg">
          <h4 className="font-headline-md text-[16px] text-on-surface mb-md">{t('tasks.taskCalendarView.activeNow')}</h4>
          <div className="space-y-md">
            {allTasks.filter(task => task.status === 'running' || task.status === 'in_progress').slice(0, 3).map(task => (
              <div key={task.id} className="flex items-start gap-md">
                <div className="w-1 bg-primary h-8 rounded-full" />
                <div>
                  <p className="text-body-sm font-semibold">{task.title}</p>
                  <p className="text-[12px] text-on-surface-variant">{task.assignee || t('tasks.taskCalendarView.unassigned')}</p>
                </div>
              </div>
            ))}
            {allTasks.filter(task => task.status === 'running' || task.status === 'in_progress').length === 0 ? (
              <p className="text-body-sm text-on-surface-variant italic opacity-60">{t('tasks.taskCalendarView.noActiveTasks')}</p>
            ) : null}
          </div>
        </div>
      </div>
    </div>
  )
}
