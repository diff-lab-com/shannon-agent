// Paginated task list with empty/loading states.
//
// MD3 tokens. Delegates each row to TaskCard. Pagination via existing
// Pagination component.

import { useIntl } from 'react-intl'
import EmptyState from '@/components/ui/empty-state'
import { Pagination } from '@/components/ui/pagination'
import { CardSkeleton } from '@/components/SkeletonLoader'
import type { TaskItem } from '@/types'
import TaskCard from './TaskCard'
import { TASKS_PER_PAGE } from './shared'

interface TaskListProps {
  tasks: TaskItem[]
  loading: boolean
  page: number
  totalPages: number
  onPageChange: (page: number) => void
  runningId: string | null
  onSelectTask: (id: string) => void
  onRunNow: (id: string) => void
  onCancelTask: (id: string) => void
}

export default function TaskList({
  tasks,
  loading,
  page,
  totalPages,
  onPageChange,
  runningId,
  onSelectTask,
  onRunNow,
  onCancelTask,
}: TaskListProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  return (
    <div className="col-span-12 lg:col-span-8 space-y-md">
      {loading ? (
        Array.from({ length: 3 }).map((_, i) => <CardSkeleton key={i} />)
      ) : tasks.length === 0 ? (
        <EmptyState icon="task_alt" title={t('tasks.taskList.empty')} />
      ) : null}

      {tasks.map(task => (
        <TaskCard
          key={task.id}
          task={task}
          isRunning={runningId === task.id}
          onSelect={() => onSelectTask(task.id)}
          onRunNow={() => onRunNow(task.id)}
          onCancel={() => onCancelTask(task.id)}
        />
      ))}

      <Pagination page={page} totalPages={totalPages} onPageChange={onPageChange} />
    </div>
  )
}

export { TASKS_PER_PAGE }
