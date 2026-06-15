// Mini month calendar shown in the Tasks page sidebar.
//
// MD3 tokens. Highlights today and days with running tasks. Also renders the
// "Active Now" list below the grid. Month navigation via chevron buttons.
//
// Scheduled-routine integration: if `scheduledTasks` is provided, days with a
// routine `next_fire_at` (rendered as a local date) are highlighted.

import type { TaskItem, ScheduledRoutine } from '@/types'
import { DAY_NAMES, MONTH_NAMES } from './shared'

interface CalendarSidebarWidgetProps {
  viewMonth: number
  viewYear: number
  onPrevMonth: () => void
  onNextMonth: () => void
  tasks: TaskItem[]
  scheduledTasks?: ScheduledRoutine[]
  onSelectTask?: (id: string) => void
  onSelectRoutine?: (id: string) => void
}

export default function CalendarSidebarWidget({
  viewMonth,
  viewYear,
  onPrevMonth,
  onNextMonth,
  tasks,
  scheduledTasks = [],
  onSelectTask,
  onSelectRoutine,
}: CalendarSidebarWidgetProps) {
  const today = new Date()
  const startDay = (new Date(viewYear, viewMonth, 1).getDay() + 6) % 7 // Monday-based
  const daysInMonth = new Date(viewYear, viewMonth + 1, 0).getDate()
  const prevMonthDays = new Date(viewYear, viewMonth, 0).getDate()

  // Collect days (1-31) that have a scheduled routine firing this month.
  const fireDays = new Set<number>()
  for (const r of scheduledTasks) {
    if (r.next_fire_at == null) continue
    const d = new Date(r.next_fire_at * 1000)
    if (d.getMonth() === viewMonth && d.getFullYear() === viewYear) {
      fireDays.add(d.getDate())
    }
  }

  const activeTasks = tasks.filter(t => t.status === 'running' || t.status === 'in_progress').slice(0, 3)
  const hasActive = tasks.some(t => t.status === 'running' || t.status === 'in_progress')

  return (
    <div className="bg-surface-container-lowest border border-outline-variant/30 rounded-2xl p-lg shadow-sm">
      <div className="flex items-center justify-between mb-lg">
        <div>
          <h4 className="font-headline-md text-[18px] text-on-surface">Schedule</h4>
          <span className="font-label-sm text-on-surface-variant">{MONTH_NAMES[viewMonth]} {viewYear}</span>
        </div>
        <div className="flex gap-sm">
          <button
            aria-label="Previous month"
            className="material-symbols-outlined text-on-surface-variant text-[20px] cursor-pointer hover:text-primary transition-colors"
            onClick={onPrevMonth}
          >
            chevron_left
          </button>
          <button
            aria-label="Next month"
            className="material-symbols-outlined text-on-surface-variant text-[20px] cursor-pointer hover:text-primary transition-colors"
            onClick={onNextMonth}
          >
            chevron_right
          </button>
        </div>
      </div>
      <div className="grid grid-cols-7 text-center mb-sm">
        {DAY_NAMES.map(d => <span key={d} className="text-[10px] font-bold text-outline uppercase">{d}</span>)}
      </div>
      <div className="grid grid-cols-7 gap-1 text-center font-label-md">
        {Array.from({ length: startDay }, (_, i) => (
          <span key={`prev-${i}`} className="py-2 text-outline/30">{prevMonthDays - startDay + i + 1}</span>
        ))}
        {Array.from({ length: daysInMonth }, (_, i) => {
          const day = i + 1
          const isToday = viewMonth === today.getMonth() && viewYear === today.getFullYear() && day === today.getDate()
          const hasFire = fireDays.has(day)
          return (
            <span
              key={day}
              className={`py-2 rounded-lg cursor-pointer relative ${isToday ? 'bg-primary text-on-primary font-bold' : hasFire ? 'bg-primary-container/20 text-primary font-bold' : 'hover:bg-surface-container'}`}
            >
              {day}
            </span>
          )
        })}
      </div>

      <div className="mt-lg pt-lg border-t border-outline-variant/20">
        <h5 className="font-label-sm text-outline uppercase tracking-wider mb-md">Active Now</h5>
        <div className="space-y-md">
          {activeTasks.map(t => (
            <div
              key={t.id}
              className={`flex items-start gap-md ${onSelectTask ? 'cursor-pointer' : ''}`}
              onClick={() => onSelectTask?.(t.id)}
            >
              <div className="w-1 bg-primary h-8 rounded-full" />
              <div>
                <p className="text-body-sm font-semibold">{t.title}</p>
                <p className="text-[12px] text-on-surface-variant">{t.assignee || 'Unassigned'}</p>
              </div>
            </div>
          ))}
          {!hasActive ? (
            <p className="text-body-sm text-on-surface-variant italic opacity-60">No active tasks</p>
          ) : null}
        </div>
      </div>

      {scheduledTasks.length > 0 && (
        <div className="mt-lg pt-lg border-t border-outline-variant/20">
          <h5 className="font-label-sm text-outline uppercase tracking-wider mb-md">Routines</h5>
          <div className="space-y-sm">
            {scheduledTasks.slice(0, 6).map(r => (
              <button
                key={r.id}
                type="button"
                className="w-full flex items-center gap-sm text-left py-xs px-sm rounded-lg hover:bg-surface-container-low/60 transition-colors cursor-pointer focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
                onClick={() => onSelectRoutine?.(r.id)}
                aria-label={`Open routine detail for ${r.name}`}
              >
                <span className="material-symbols-outlined text-[16px] text-primary">
                  {r.enabled ? 'bolt' : 'block'}
                </span>
                <span className="flex-1 min-w-0">
                  <span className="block text-body-sm font-semibold text-on-surface truncate">
                    {r.name}
                  </span>
                  <span className="block text-[11px] text-on-surface-variant uppercase tracking-wider">
                    {r.trigger_type}
                    {r.depends_on && r.depends_on.length > 0
                      ? ` · ${r.depends_on.length} dep${r.depends_on.length === 1 ? '' : 's'}`
                      : ''}
                  </span>
                </span>
                <span className="material-symbols-outlined text-[14px] text-on-surface-variant">chevron_right</span>
              </button>
            ))}
          </div>
        </div>
      )}
    </div>
  )
}
