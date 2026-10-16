// Mini month calendar card (design 04-tasks 右列上卡, G4a).
//
// Current-month grid only — today wears the solid violet circle, days with a
// scheduled routine firing (backend `next_fire_at`, rendered as a local
// date) carry a small violet dot, and the foot states the scheduled count.
// (Routine dots ride the ALREADY-resolved next_fire_at field — no cron
// parsing frontend-side; a routine without a next fire shows no dot.)
//
// The old card's Active-Now / routine quick-lists moved out with the G4a
// redesign: the design's compact card is calendar-only (runs live in the
// Runs tab, routines in the Routines tab — both one click away).

import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import type { ScheduledRoutine } from '@/types'
import { monthName, weekdayName } from './shared'
import { cn } from '@/lib/utils'

interface CalendarSidebarWidgetProps {
  /** Opens the create-schedule form (the header + button). */
  onCreateScheduled?: () => void
  scheduledTasks?: ScheduledRoutine[]
}

export default function CalendarSidebarWidget({
  onCreateScheduled,
  scheduledTasks = [],
}: CalendarSidebarWidgetProps) {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, string | number>) =>
    intl.formatMessage({ id }, values)

  const today = new Date()
  const viewMonth = today.getMonth()
  const viewYear = today.getFullYear()
  const startDay = (new Date(viewYear, viewMonth, 1).getDay() + 6) % 7 // Monday-based
  const daysInMonth = new Date(viewYear, viewMonth + 1, 0).getDate()
  const prevMonthDays = new Date(viewYear, viewMonth, 0).getDate()

  // Days (1-31) with a scheduled routine firing this month.
  const fireDays = new Set<number>()
  for (const r of scheduledTasks) {
    if (r.next_fire_at == null) continue
    const d = new Date(r.next_fire_at * 1000)
    if (d.getMonth() === viewMonth && d.getFullYear() === viewYear) {
      fireDays.add(d.getDate())
    }
  }

  return (
    <div
      className="bg-surface-container-lowest border border-outline-variant/30 rounded-2xl p-lg shadow-e1"
      data-testid="tasks-side-calendar"
    >
      <div className="flex items-center justify-between mb-lg">
        <div className="flex items-center gap-xs min-w-0">
          <span className="material-symbols-outlined icon-md text-on-surface-variant" aria-hidden="true">calendar_month</span>
          <h4 className="font-headline-md text-body-lg text-on-surface">
            {monthName(intl.locale, viewMonth)}
          </h4>
          <span className="font-label-sm text-on-surface-variant tabular-nums">{viewYear}</span>
        </div>
        {onCreateScheduled && (
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={t('tasks.sideCalendar.newAria')}
            title={t('tasks.sideCalendar.newAria')}
            className="text-on-surface-variant hover:text-primary"
            onClick={onCreateScheduled}
          >
            <span className="material-symbols-outlined icon-md">add</span>
          </Button>
        )}
      </div>
      <div className="grid grid-cols-7 text-center mb-sm">
        {[1, 2, 3, 4, 5, 6, 0].map(jsDay => <span key={jsDay} className="text-label-2xs font-bold text-on-surface-variant uppercase">{weekdayName(intl.locale, jsDay, 'short')}</span>)}
      </div>
      <div className="grid grid-cols-7 gap-xs text-center font-label-md">
        {Array.from({ length: startDay }, (_, i) => (
          <span key={`prev-${i}`} aria-hidden="true" className="py-sm text-on-surface-variant">{prevMonthDays - startDay + i + 1}</span>
        ))}
        {Array.from({ length: daysInMonth }, (_, i) => {
          const day = i + 1
          const isToday = day === today.getDate()
          const hasFire = fireDays.has(day)
          return (
            <span key={day} className="relative py-sm rounded-full flex items-center justify-center">
              {/* Today = the solid violet circle (design 04:670 「7」). */}
              <span
                aria-current={isToday ? 'date' : undefined}
                className={cn(
                  'w-7 h-7 rounded-full flex items-center justify-center tabular-nums',
                  isToday ? 'bg-primary text-on-primary font-bold' : 'text-on-surface',
                )}
              >
                {day}
              </span>
              {/* Scheduled-routine dot sits under the number. */}
              {hasFire && (
                <span
                  aria-hidden="true"
                  data-testid="tasks-calendar-fire-dot"
                  className={cn(
                    'absolute bottom-[2px] left-1/2 -translate-x-1/2 w-1 h-1 rounded-full',
                    isToday ? 'bg-on-primary' : 'bg-primary',
                  )}
                />
              )}
            </span>
          )
        })}
      </div>

      {/* Scheduled-count legend (design 04:537). */}
      <div className="mt-md pt-md border-t border-outline-variant/20 flex items-center gap-xs">
        <span aria-hidden="true" className="w-1 h-1 rounded-full bg-primary shrink-0" />
        <span className="font-label-sm text-on-surface-variant" data-testid="tasks-calendar-scheduled-count">
          {t('tasks.sideCalendar.scheduled', { count: fireDays.size })}
        </span>
      </div>
    </div>
  )
}
