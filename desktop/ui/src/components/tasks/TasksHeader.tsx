// Tasks page header — action cluster. The page-level h1/h2 was retired
// (P0-3); the global app Header carries the page name, and the page-level
// subtitle lives one level down. This component is the toolbar only.
//
// 2026-09 Q1+Q2 (review): in Simple mode the header should look like a
// flat "+ New background task" button — no team filter, no view toggles,
// no nested split menu. Dev mode exposes the full toolbar.

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
  /** Secondary create entry: one-off background task (NewTaskForm). */
  onToggleNewTask: () => void
  /** P1-2: optional best-of-N batch entry (shown when provided). */
  onToggleBatch?: () => void
  /** IA T4: the primary CTA — creates a scheduled automation (ScheduleForm). */
  onToggleSchedule: () => void
  /** G11: unique team names available for filtering. */
  teams?: string[]
  /** Current team filter value ('all' or a specific team name). */
  teamFilter?: string
  onTeamFilterChange?: (team: string) => void
  /** Simple (default) hides all the developer surfaces. */
  mode?: 'simple' | 'dev'
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
  mode = 'simple',
}: TasksHeaderProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const [newMenuOpen, setNewMenuOpen] = useState(false)

  // IA T4 (页面归位): one primary CTA —「新建自动化」(ScheduleForm). The
  // one-off background task and best-of-N batch entries fold into the
  // split-button dropdown (same pattern as the Sidebar「新建」split), so
  // the page reads a single hierarchy instead of sibling buttons.
  const newMenuItems: DropdownMenuItem[] = [
    {
      id: 'new-task',
      label: t('tasks.tasksHeader.newBackgroundTask'),
      icon: 'add_task',
      onSelect: onToggleNewTask,
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

  // The create cluster is identical in both modes: primary「新建自动化」+
  // caret with the secondary entries.
  const createCluster = (
    <div className="flex items-stretch">
      <Button
        aria-label={t('tasks.tasksHeader.newAutomation')}
        className="px-md py-sm bg-primary text-on-primary rounded-l-xl flex items-center gap-sm font-label-md cursor-pointer hover:shadow-md active:scale-95 transition-all"
        onClick={onToggleSchedule}
      >
        <span className="material-symbols-outlined icon-md">add</span>
        {t('tasks.tasksHeader.newAutomation')}
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
  )

  // Simple mode (default) — a one-line subtitle plus the「新建自动化」
  // split button. Anything else (team filter, view toggles) is dev-only;
  // first-run users shouldn't have to learn the task-ops taxonomy to find
  // the single primary action.
  if (mode === 'simple') {
    return (
      <div className="flex items-center justify-between gap-md mb-lg flex-wrap">
        <p className="text-on-surface-variant font-body-sm">{t('tasks.tasksHeader.subtitle')}</p>
        {createCluster}
      </div>
    )
  }

  return (
    <div className="flex items-center gap-sm flex-wrap mb-xl">
        {/* ── create ─────────────────────────────────────────────────── */}
        {createCluster}

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
  )
}
