// Right-side drawer showing task details with inline editing for G9.
//
// MD3 tokens. Click backdrop or close button to dismiss. Handles both
// TaskItem and BackgroundTaskInfo shapes via duck-typing. TaskItem rows
// (id/title/status/description) support inline editing of priority,
// assignee, status, and due date through the `update_task` Tauri command.

import { useEffect, useState } from 'react'
import { toast } from 'sonner'
import type { TaskItem, BackgroundTaskInfo, UpdateTaskPayload } from '@/types'
import * as api from '@/lib/tauri-api'

type TaskLike = TaskItem | BackgroundTaskInfo

interface TaskDetailDrawerProps {
  task: TaskLike | null
  onClose: () => void
  /** Optional callback after a successful update so parent can refresh. */
  onUpdated?: () => void
}

const PRIORITIES = ['low', 'normal', 'high', 'critical'] as const
const STATUSES = ['pending', 'in_progress', 'running', 'completed', 'failed', 'blocked'] as const

function getTitle(task: TaskLike): string {
  if ('title' in task) return task.title
  return task.prompt?.slice(0, 80) ?? 'Background Task'
}

function isTaskItem(t: TaskLike): t is TaskItem {
  return 'title' in t
}

function toDateInputValue(ts: number | null | undefined): string {
  if (!ts) return ''
  const d = new Date(ts * 1000)
  if (Number.isNaN(d.getTime())) return ''
  // yyyy-mm-dd in local time
  const yyyy = d.getFullYear()
  const mm = String(d.getMonth() + 1).padStart(2, '0')
  const dd = String(d.getDate()).padStart(2, '0')
  return `${yyyy}-${mm}-${dd}`
}

function fromDateInputValue(s: string): number | null {
  if (!s) return null
  const d = new Date(`${s}T00:00:00`)
  if (Number.isNaN(d.getTime())) return null
  return Math.floor(d.getTime() / 1000)
}

export default function TaskDetailDrawer({ task, onClose, onUpdated }: TaskDetailDrawerProps) {
  const [editing, setEditing] = useState(false)
  const [status, setStatus] = useState('')
  const [assignee, setAssignee] = useState('')
  const [priority, setPriority] = useState('')
  const [dueDate, setDueDate] = useState('')
  const [saving, setSaving] = useState(false)

  useEffect(() => {
    if (!task) return
    setEditing(false)
    if (isTaskItem(task)) {
      setStatus(task.status ?? '')
      setAssignee(task.assignee ?? '')
      setPriority(task.priority ?? '')
      setDueDate(toDateInputValue(task.due_date ?? null))
    }
  }, [task])

  if (!task) return null

  const editable = isTaskItem(task)

  const handleSave = async () => {
    if (!editable) return
    setSaving(true)
    const payload: UpdateTaskPayload = {
      id: task.id,
      status: status || undefined,
      assignee: assignee.trim() || undefined,
      priority: priority || undefined,
      due_date: fromDateInputValue(dueDate),
    }
    try {
      await api.updateTask(payload)
      toast.success('Task updated')
      setEditing(false)
      onUpdated?.()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to update task')
    } finally {
      setSaving(false)
    }
  }

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

          {/* Status */}
          <div>
            <span className="text-label-sm text-on-surface-variant">Status</span>
            {editing && editable ? (
              <select
                value={status}
                onChange={e => setStatus(e.target.value)}
                aria-label="Status"
                className="mt-xs w-full px-md py-xs rounded-lg border border-outline-variant/50 bg-surface-container-lowest font-body-md text-on-surface focus:outline-none focus:border-primary"
              >
                {STATUSES.map(s => (
                  <option key={s} value={s}>{s}</option>
                ))}
              </select>
            ) : (
              <p className="font-body-md text-on-surface mt-xs capitalize">{task.status}</p>
            )}
          </div>

          {'description' in task && task.description && (
            <div>
              <span className="text-label-sm text-on-surface-variant">Description</span>
              <p className="font-body-md text-on-surface mt-xs">{task.description}</p>
            </div>
          )}

          {/* Priority (editable) */}
          {editable && (
            <div>
              <span className="text-label-sm text-on-surface-variant">Priority</span>
              {editing ? (
                <select
                  value={priority}
                  onChange={e => setPriority(e.target.value)}
                  aria-label="Priority"
                  className="mt-xs w-full px-md py-xs rounded-lg border border-outline-variant/50 bg-surface-container-lowest font-body-md text-on-surface focus:outline-none focus:border-primary"
                >
                  <option value="">— none —</option>
                  {PRIORITIES.map(p => (
                    <option key={p} value={p}>{p}</option>
                  ))}
                </select>
              ) : (
                <p className="font-body-md text-on-surface mt-xs capitalize">
                  {task.priority ?? '—'}
                </p>
              )}
            </div>
          )}

          {/* Assignee (editable) */}
          {editable && (
            <div>
              <span className="text-label-sm text-on-surface-variant">Assignee</span>
              {editing ? (
                <input
                  type="text"
                  value={assignee}
                  onChange={e => setAssignee(e.target.value)}
                  placeholder="agent name"
                  aria-label="Assignee"
                  className="mt-xs w-full px-md py-xs rounded-lg border border-outline-variant/50 bg-surface-container-lowest font-body-md text-on-surface focus:outline-none focus:border-primary"
                />
              ) : (
                <p className="font-body-md text-on-surface mt-xs">{task.assignee ?? '—'}</p>
              )}
            </div>
          )}

          {/* Due date (editable, G9) */}
          {editable && (
            <div>
              <span className="text-label-sm text-on-surface-variant">Due Date</span>
              {editing ? (
                <input
                  type="date"
                  value={dueDate}
                  onChange={e => setDueDate(e.target.value)}
                  aria-label="Due date"
                  className="mt-xs w-full px-md py-xs rounded-lg border border-outline-variant/50 bg-surface-container-lowest font-body-md text-on-surface focus:outline-none focus:border-primary"
                />
              ) : (
                <p className="font-body-md text-on-surface mt-xs">
                  {task.due_date
                    ? new Date(task.due_date * 1000).toLocaleDateString()
                    : '—'}
                </p>
              )}
            </div>
          )}

          {/* Dependencies (read-only — editing happens via DependsOnEditor) */}
          {editable && (task.blocked_by?.length || task.blocks?.length) ? (
            <div>
              <span className="text-label-sm text-on-surface-variant">Dependencies</span>
              <div className="mt-xs flex flex-col gap-xs">
                {task.blocked_by?.length ? (
                  <div className="text-body-sm text-on-surface-variant">
                    Blocked by:{' '}
                    <span className="text-on-surface">{task.blocked_by.join(', ')}</span>
                  </div>
                ) : null}
                {task.blocks?.length ? (
                  <div className="text-body-sm text-on-surface-variant">
                    Blocks:{' '}
                    <span className="text-on-surface">{task.blocks.join(', ')}</span>
                  </div>
                ) : null}
              </div>
            </div>
          ) : null}

          {/* Edit / Save buttons */}
          {editable && (
            <div className="flex justify-end gap-sm pt-md border-t border-outline-variant/20">
              {editing ? (
                <>
                  <button
                    onClick={() => {
                      setEditing(false)
                      // reset to current task values
                      setStatus(task.status ?? '')
                      setAssignee(task.assignee ?? '')
                      setPriority(task.priority ?? '')
                      setDueDate(toDateInputValue(task.due_date ?? null))
                    }}
                    disabled={saving}
                    className="px-md py-xs rounded-lg text-on-surface-variant font-label-md hover:bg-surface-container cursor-pointer focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 disabled:opacity-50"
                  >
                    Cancel
                  </button>
                  <button
                    onClick={() => void handleSave()}
                    disabled={saving}
                    className="px-md py-xs rounded-lg bg-primary text-on-primary font-label-md hover:brightness-110 transition-all cursor-pointer focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 disabled:opacity-50"
                  >
                    {saving ? 'Saving…' : 'Save'}
                  </button>
                </>
              ) : (
                <button
                  onClick={() => setEditing(true)}
                  className="px-md py-xs rounded-lg bg-primary/10 text-primary font-label-md hover:bg-primary/20 transition-colors cursor-pointer focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
                >
                  <span className="material-symbols-outlined text-[16px] align-middle mr-xs">edit</span>
                  Edit
                </button>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  )
}
