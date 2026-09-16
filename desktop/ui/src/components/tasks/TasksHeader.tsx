// Tasks page header — title, subtitle, and the action cluster.
//
// Review 2026-09-16 (UI-review §17): the header used to show seven equally
// weighted buttons, burying the primary action. Now grouped by purpose and
// separated by dividers:
//   [ 新建后台任务 (primary) | ▾ 例行任务 / 多方案对比 ]  ─ create (split button)
//   [ 团队 select ] [ 筛选 ]                              ─ filter
//   [ 月历 ] [ 关系图 ]                                    ─ view toggles
// MD3 tokens. Toggle active state uses ring-2 ring-primary.

import { useState } from 'react'
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import { DropdownMenu, type DropdownMenuItem } from '@/components/ui/dropdown-menu'
import { cn } from '@/lib/utils'

interface TasksHeaderProps {
  showFilters: boolean
  onToggleFilters: () => void
  calendarView: boolean
  onToggleCalendar: () => void
  dagView?: boolean
  onToggleDag?: () => void
  onToggleNewTask: () => void
  /** P1-2: optional best-of-N batch entry (shown when provided). */
  onToggleBatch?: () => void
  onToggleSchedule: () => void
  /** G11: unique team names available for filtering. */
  teams?: string[]
  /** Current team filter value ('all' or a specific team name). */
  teamFilter?: string
  onTeamFilterChange?: (team: string) => void
}

const toggleClass = (active: boolean) =>
  cn(
    'px-md py-sm border border-outline-variant bg-surface-container-lowest text-on-surface rounded-xl flex items-center gap-sm font-label-md cursor-pointer hover:bg-surface-container transition-colors',
    active ? 'ring-2 ring-primary' : '',
  )

const Divider = () => <div aria-hidden="true" className="hidden sm:block w-px self-stretch my-sm bg-outline-variant/40" />

export default function TasksHeader({
  showFilters,
  onToggleFilters,
  calendarView,
  onToggleCalendar,
  dagView,
  onToggleDag,
  onToggleNewTask,
  onToggleBatch,
  onToggleSchedule,
  teams,
  teamFilter,
  onTeamFilterChange,
}: TasksHeaderProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const [newMenuOpen, setNewMenuOpen] = useState(false)

  const newMenuItems: DropdownMenuItem[] = [
    {
      id: 'new-routine',
      label: t('tasks.tasksHeader.newRoutine'),
      icon: 'schedule',
      onSelect: onToggleSchedule,
    },
    ...(onToggleBatch
      ? [{
          id: 'new-batch',
          label: t('batch.form.headerButton'),
          icon: 'call_split',
          onSelect: onToggleBatch,
        }]
      : []),
  ]

  return (
    <div className="flex flex-col md:flex-row md:items-end justify-between mb-xl gap-md">
      <div>
        <h2 className="font-headline-lg text-headline-lg text-on-surface">{t('tasks.tasksHeader.title')}</h2>
        <p className="text-on-surface-variant mt-xs">{t('tasks.tasksHeader.subtitle')}</p>
      </div>
      <div className="flex items-center gap-sm flex-wrap">
        {/* ── create ─────────────────────────────────────────────────── */}
        <div className="flex items-stretch">
          <Button
            aria-label={t('tasks.tasksHeader.newBackgroundTask')}
            className="px-md py-sm bg-primary text-on-primary rounded-l-xl flex items-center gap-sm font-label-md cursor-pointer hover:shadow-md active:scale-95 transition-all"
            onClick={onToggleNewTask}
          >
            <span className="material-symbols-outlined icon-md">add</span>
            {t('tasks.tasksHeader.newBackgroundTask')}
          </Button>
          <span className="relative flex items-stretch">
            <button
              type="button"
              aria-haspopup="menu"
              aria-expanded={newMenuOpen}
              aria-label={t('tasks.tasksHeader.newMore.aria')}
              className="px-sm bg-primary text-on-primary rounded-r-xl border-l border-on-primary/25 cursor-pointer hover:shadow-md active:scale-95 transition-all flex items-center"
              onClick={() => setNewMenuOpen(open => !open)}
            >
              <span className="material-symbols-outlined icon-md" aria-hidden="true">expand_more</span>
            </button>
            <DropdownMenu
              open={newMenuOpen}
              onClose={() => setNewMenuOpen(false)}
              items={newMenuItems}
              ariaLabel={t('tasks.tasksHeader.newMore.aria')}
            />
          </span>
        </div>

        <Divider />

        {/* ── filter ─────────────────────────────────────────────────── */}
        {teams && teams.length > 0 && onTeamFilterChange ? (
          <label className="flex items-center gap-xs px-md py-sm border border-outline-variant bg-surface-container-lowest text-on-surface rounded-xl font-label-md">
            <span className="material-symbols-outlined text-[18px] text-on-surface-variant">groups</span>
            <span className="sr-only">{t('tasks.tasksHeader.filterByTeam')}</span>
            <select
              aria-label={t('tasks.tasksHeader.filterByTeam')}
              value={teamFilter ?? 'all'}
              onChange={e => onTeamFilterChange(e.target.value)}
              className="bg-transparent border-none focus:outline-none cursor-pointer text-on-surface"
            >
              <option value="all">{t('tasks.tasksHeader.allTeams')}</option>
              {teams.map(t => (
                <option key={t} value={t}>{t}</option>
              ))}
            </select>
          </label>
        ) : null}
        <Button
          aria-label={t('tasks.tasksHeader.filters')}
          onClick={onToggleFilters}
          className={toggleClass(showFilters)}
        >
          <span className="material-symbols-outlined text-[18px]">filter_list</span>
          {t('tasks.tasksHeader.filters')}
        </Button>

        <Divider />

        {/* ── view toggles ───────────────────────────────────────────── */}
        <Button
          aria-label={t('tasks.tasksHeader.monthView')}
          onClick={onToggleCalendar}
          className={toggleClass(calendarView)}
        >
          <span className="material-symbols-outlined text-[18px]">calendar_month</span>
          {calendarView ? t('tasks.tasksHeader.listView') : t('tasks.tasksHeader.monthView')}
        </Button>
        {onToggleDag ? (
          <Button
            aria-label={t('tasks.tasksHeader.graph')}
            onClick={onToggleDag}
            className={toggleClass(dagView ?? false)}
          >
            <span className="material-symbols-outlined text-[18px]">account_tree</span>
            {dagView ? t('tasks.tasksHeader.hideGraph') : t('tasks.tasksHeader.graph')}
          </Button>
        ) : null}
      </div>
    </div>
  )
}
