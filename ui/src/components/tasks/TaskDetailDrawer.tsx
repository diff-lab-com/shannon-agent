// Right-side drawer showing task details (title, status, description, etc.).
//
// MD3 tokens. Click backdrop or close button to dismiss. Handles both
// TaskItem and BackgroundTaskInfo shapes via duck-typing.

import type { TaskItem, BackgroundTaskInfo } from '@/types'

type TaskLike = TaskItem | BackgroundTaskInfo

interface TaskDetailDrawerProps {
  task: TaskLike | null
  onClose: () => void
}

function getTitle(task: TaskLike): string {
  if ('title' in task) return task.title
  return task.prompt?.slice(0, 80) ?? 'Background Task'
}

export default function TaskDetailDrawer({ task, onClose }: TaskDetailDrawerProps) {
  if (!task) return null
  return (
    <div
      className="fixed inset-0 z-50 flex justify-end"
      onClick={onClose}
      onKeyDown={e => { if (e.key === 'Escape') onClose() }}
    >
      <div className="bg-black/20 absolute inset-0" />
      <div
        className="relative w-[400px] bg-surface-container-lowest shadow-2xl border-l border-outline-variant/20 p-xl overflow-y-auto"
        onClick={e => e.stopPropagation()}
      >
        <div className="flex items-center justify-between mb-lg">
          <h3 className="font-headline-md text-on-surface font-bold">Task Detail</h3>
          <button
            aria-label="Close drawer"
            className="p-sm rounded-lg hover:bg-surface-container text-on-surface-variant cursor-pointer"
            onClick={onClose}
          >
            <span className="material-symbols-outlined">close</span>
          </button>
        </div>
        <div className="space-y-md">
          <div>
            <span className="text-label-sm text-on-surface-variant">Title</span>
            <p className="font-body-lg text-on-surface font-bold mt-xs">{getTitle(task)}</p>
          </div>
          <div>
            <span className="text-label-sm text-on-surface-variant">Status</span>
            <p className="font-body-md text-on-surface mt-xs capitalize">{task.status}</p>
          </div>
          {'description' in task && task.description && (
            <div>
              <span className="text-label-sm text-on-surface-variant">Description</span>
              <p className="font-body-md text-on-surface mt-xs">{task.description}</p>
            </div>
          )}
          {'priority' in task && task.priority && (
            <div>
              <span className="text-label-sm text-on-surface-variant">Priority</span>
              <p className="font-body-md text-on-surface mt-xs capitalize">{task.priority}</p>
            </div>
          )}
          {'assignee' in task && task.assignee && (
            <div>
              <span className="text-label-sm text-on-surface-variant">Assignee</span>
              <p className="font-body-md text-on-surface mt-xs">{task.assignee}</p>
            </div>
          )}
        </div>
      </div>
    </div>
  )
}
