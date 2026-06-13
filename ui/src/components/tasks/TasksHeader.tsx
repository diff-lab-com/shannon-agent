// Tasks page header — title, subtitle, and action buttons (Filters, Month/List,
// New Background Task).
//
// MD3 tokens. Button active state uses ring-2 ring-primary.

import { Button } from '@/components/ui/button'

interface TasksHeaderProps {
  showFilters: boolean
  onToggleFilters: () => void
  calendarView: boolean
  onToggleCalendar: () => void
  dagView?: boolean
  onToggleDag?: () => void
  onToggleNewTask: () => void
  onToggleSchedule: () => void
}

export default function TasksHeader({
  showFilters,
  onToggleFilters,
  calendarView,
  onToggleCalendar,
  dagView,
  onToggleDag,
  onToggleNewTask,
  onToggleSchedule,
}: TasksHeaderProps) {
  return (
    <div className="flex flex-col md:flex-row md:items-end justify-between mb-xl gap-md">
      <div>
        <h2 className="font-headline-lg text-headline-lg text-on-surface">Scheduled Tasks</h2>
        <p className="text-on-surface-variant mt-xs">Manage and monitor your automated intelligence workflows.</p>
      </div>
      <div className="flex gap-sm">
        <Button
          aria-label="Toggle filters"
          onClick={onToggleFilters}
          className={`px-md py-sm border border-outline-variant bg-surface-container-lowest text-on-surface rounded-xl flex items-center gap-sm font-label-md cursor-pointer hover:bg-surface-container transition-colors ${showFilters ? 'ring-2 ring-primary' : ''}`}
        >
          <span className="material-symbols-outlined text-[18px]">filter_list</span>
          Filters
        </Button>
        <Button
          aria-label="Toggle calendar view"
          onClick={onToggleCalendar}
          className={`px-md py-sm border border-outline-variant bg-surface-container-lowest text-on-surface rounded-xl flex items-center gap-sm font-label-md cursor-pointer hover:bg-surface-container transition-colors ${calendarView ? 'ring-2 ring-primary' : ''}`}
        >
          <span className="material-symbols-outlined text-[18px]">calendar_month</span>
          {calendarView ? 'List View' : 'Month View'}
        </Button>
        {onToggleDag ? (
          <Button
            aria-label="Toggle graph view"
            onClick={onToggleDag}
            className={`px-md py-sm border border-outline-variant bg-surface-container-lowest text-on-surface rounded-xl flex items-center gap-sm font-label-md cursor-pointer hover:bg-surface-container transition-colors ${dagView ? 'ring-2 ring-primary' : ''}`}
          >
            <span className="material-symbols-outlined text-[18px]">account_tree</span>
            {dagView ? 'Hide Graph' : 'Graph'}
          </Button>
        ) : null}
        <Button
          aria-label="Create new scheduled routine"
          className="px-md py-sm border border-outline-variant bg-surface-container-lowest text-on-surface rounded-xl flex items-center gap-sm font-label-md cursor-pointer hover:bg-surface-container transition-colors"
          onClick={onToggleSchedule}
        >
          <span className="material-symbols-outlined text-[18px]">schedule</span>
          New Routine
        </Button>
        <Button
          aria-label="Create new task"
          className="px-md py-sm bg-primary text-on-primary rounded-xl flex items-center gap-sm font-label-md cursor-pointer hover:shadow-md active:scale-95 transition-all"
          onClick={onToggleNewTask}
        >
          <span className="material-symbols-outlined text-[20px]">add</span>
          New Background Task
        </Button>
      </div>
    </div>
  )
}
