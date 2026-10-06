// Status filter chips row — toggles between All / Pending / Running / Completed.
//
// MD3 tokens. Active chip uses bg-primary-container text-on-primary-container
// font-bold (G7 2026-09-30: accent-tint chips `bg-primary/10 text-primary`
// failed AA on the non-default light themes — see check-design-tokens guard).

import { Button } from '@/components/ui/button'
import { useIntl } from 'react-intl'
import type { FilterStatus } from './shared'
import { cn } from '@/lib/utils'

interface TasksFiltersProps {
  active: FilterStatus
  onChange: (value: FilterStatus) => void
}

const OPTIONS: ReadonlyArray<[FilterStatus, string]> = [
  ['all', 'all'],
  ['pending', 'pending'],
  ['running', 'running'],
  ['completed', 'completed'],
]

export default function TasksFilters({ active, onChange }: TasksFiltersProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  return (
    <div className="flex gap-sm mb-lg flex-wrap">
      {OPTIONS.map(([value, label]) => (
        <Button
          key={value}
          variant="ghost"
          onClick={() => onChange(value)}
          className={cn('px-sm py-xs rounded-full text-label-sm transition-colors cursor-pointer', active === value ? 'bg-primary-container text-on-primary-container font-bold' : 'bg-surface-container-low text-on-surface-variant hover:text-primary hover:bg-primary/10')}
        >
          {t(`tasks.tasksFilters.${label}`)}
        </Button>
      ))}
    </div>
  )
}
