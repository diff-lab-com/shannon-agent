// Status filter chips row — toggles between All / Pending / Running / Completed.
//
// MD3 tokens. Active chip uses bg-primary/10 text-primary font-bold.

import { Button } from '@/components/ui/button'
import type { FilterStatus } from './shared'

interface TasksFiltersProps {
  active: FilterStatus
  onChange: (value: FilterStatus) => void
}

const OPTIONS: ReadonlyArray<[FilterStatus, string]> = [
  ['all', 'All'],
  ['pending', 'Pending'],
  ['running', 'Running'],
  ['completed', 'Completed'],
]

export default function TasksFilters({ active, onChange }: TasksFiltersProps) {
  return (
    <div className="flex gap-sm mb-lg flex-wrap">
      {OPTIONS.map(([value, label]) => (
        <Button
          key={value}
          variant="ghost"
          onClick={() => onChange(value)}
          className={`px-sm py-xs rounded-full text-label-sm transition-colors cursor-pointer ${active === value ? 'bg-primary/10 text-primary font-bold' : 'bg-surface-container-low text-on-surface-variant hover:text-primary hover:bg-primary/10'}`}
        >
          {label}
        </Button>
      ))}
    </div>
  )
}
