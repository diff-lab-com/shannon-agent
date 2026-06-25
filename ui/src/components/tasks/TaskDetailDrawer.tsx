// Right-side drawer showing task details with inline editing for G9.
//
// MD3 tokens. Click backdrop or close button to dismiss. Handles both
// TaskItem and BackgroundTaskInfo shapes via duck-typing. TaskItem rows
// (id/title/status/description) support inline editing of priority,
// assignee, status, and due date through the `update_task` Tauri command.

import { useEffect, useMemo, useState } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import type { TaskItem, BackgroundTaskInfo, UpdateTaskPayload } from '@/types'
import * as api from '@/lib/tauri-api'
import { useApp } from '@/context/AppContext'
import { normalizePriority } from '@/lib/task-status'

type TaskLike = TaskItem | BackgroundTaskInfo

interface TaskDetailDrawerProps {
  task: TaskLike | null
  onClose: () => void
  /** Optional callback after a successful update so parent can refresh. */
  onUpdated?: () => void
}

const PRIORITIES = ['low', 'normal', 'medium', 'high', 'critical'] as const
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
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const { agents } = useApp()
  const [editing, setEditing] = useState(false)
  const [status, setStatus] = useState('')
  const [assignee, setAssignee] = useState('')
  const [priority, setPriority] = useState('')
  const [dueDate, setDueDate] = useState('')
  const [executionMode, setExecutionMode] = useState<'serial' | 'parallel'>('serial')
  const [saving, setSaving] = useState(false)

  // G8: build a sorted unique list of assignable agent names. Lets the user
  // pick from existing agents while still allowing ad-hoc names via free text.
  const agentNames = useMemo(() => {
    const names = new Set<string>()
    for (const a of agents) {
      if (a.name) names.add(a.name)
    }
    return Array.from(names).sort()
  }, [agents])

  useEffect(() => {
    if (!task) return
    setEditing(false)
    if (isTaskItem(task)) {
      setStatus(task.status ?? '')
      setAssignee(task.assignee ?? '')
      setPriority(task.priority ?? '')
      setDueDate(toDateInputValue(task.due_date ?? null))
      setExecutionMode(task.execution_mode === 'parallel' ? 'parallel' : 'serial')
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
      execution_mode: executionMode,
    }
    try {
      await api.updateTask(payload)
      toast.success(t('tasks.taskDetailDrawer.taskUpdated'))
      setEditing(false)
      onUpdated?.()
    } catch (e) {
      toast.error(e instanceof Error ? e.message : t('tasks.taskDetailDrawer.updateFailed'))
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
          <h3 className="font-headline-md text-on-surface font-bold">{t('tasks.taskDetailDrawer.title')}</h3>
          <button
            aria-label={t('tasks.taskDetailDrawer.closeAria')}
            className="p-sm rounded-lg hover:bg-surface-container text-on-surface-variant cursor-pointer"
            onClick={onClose}
          >
            <span className="material-symbols-outlined">close</span>
          </button>
        </div>
        <div className="space-y-md">
          <div>
            <span className="text-label-sm text-on-surface-variant">{t('tasks.taskDetailDrawer.titleLabel')}</span>
            <p className="font-body-lg text-on-surface font-bold mt-xs">{getTitle(task)}</p>
          </div>

          {/* Status */}
          <div>
            <span className="text-label-sm text-on-surface-variant">{t('tasks.taskDetailDrawer.status')}</span>
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
              <span className="text-label-sm text-on-surface-variant">{t('tasks.taskDetailDrawer.description')}</span>
              <p className="font-body-md text-on-surface mt-xs">{task.description}</p>
            </div>
          )}

          {/* Priority (editable) */}
          {editable && (
            <div>
              <span className="text-label-sm text-on-surface-variant">{t('tasks.taskDetailDrawer.priority')}</span>
              {editing ? (
                <select
                  value={priority}
                  onChange={e => setPriority(e.target.value)}
                  aria-label={t('tasks.taskDetailDrawer.priority')}
                  className="mt-xs w-full px-md py-xs rounded-lg border border-outline-variant/50 bg-surface-container-lowest font-body-md text-on-surface focus:outline-none focus:border-primary"
                >
                  <option value="">{t('tasks.taskDetailDrawer.none')}</option>
                  {PRIORITIES.map(p => (
                    <option key={p} value={p}>{p}</option>
                  ))}
                </select>
              ) : (
                <p className="font-body-md text-on-surface mt-xs capitalize">
                  {(task.priority && normalizePriority(task.priority)) ?? t('tasks.taskDetailDrawer.none')}
                </p>
              )}
            </div>
          )}

          {/* Assignee (editable, G8 — datalist of known agents + free text) */}
          {editable && (
            <div>
              <span className="text-label-sm text-on-surface-variant">{t('tasks.taskDetailDrawer.assignee')}</span>
              {editing ? (
                <>
                  <input
                    type="text"
                    list="assignee-options"
                    value={assignee}
                    onChange={e => setAssignee(e.target.value)}
                    placeholder={t('tasks.taskDetailDrawer.assigneePlaceholder')}
                    aria-label={t('tasks.taskDetailDrawer.assignee')}
                    className="mt-xs w-full px-md py-xs rounded-lg border border-outline-variant/50 bg-surface-container-lowest font-body-md text-on-surface focus:outline-none focus:border-primary"
                  />
                  <datalist id="assignee-options">
                    {agentNames.map(n => (
                      <option key={n} value={n} />
                    ))}
                  </datalist>
                </>
              ) : (
                <p className="font-body-md text-on-surface mt-xs">{task.assignee ?? t('tasks.taskDetailDrawer.none')}</p>
              )}
            </div>
          )}

          {/* Due date (editable, G9) */}
          {editable && (
            <div>
              <span className="text-label-sm text-on-surface-variant">{t('tasks.taskDetailDrawer.dueDate')}</span>
              {editing ? (
                <input
                  type="date"
                  value={dueDate}
                  onChange={e => setDueDate(e.target.value)}
                  aria-label={t('tasks.taskDetailDrawer.dueDate')}
                  className="mt-xs w-full px-md py-xs rounded-lg border border-outline-variant/50 bg-surface-container-lowest font-body-md text-on-surface focus:outline-none focus:border-primary"
                />
              ) : (
                <p className="font-body-md text-on-surface mt-xs">
                  {task.due_date
                    ? new Date(task.due_date * 1000).toLocaleDateString()
                    : t('tasks.taskDetailDrawer.none')}
                </p>
              )}
            </div>
          )}

          {/* Execution mode (editable, G7 — controls how `blocks` schedule) */}
          {editable && (
            <div>
              <span className="text-label-sm text-on-surface-variant">{t('tasks.taskDetailDrawer.execution')}</span>
              {editing ? (
                <div
                  role="radiogroup"
                  aria-label={t('tasks.taskDetailDrawer.execution')}
                  className="mt-xs flex gap-sm"
                >
                  {(['serial', 'parallel'] as const).map(mode => {
                    const active = executionMode === mode
                    return (
                      <button
                        key={mode}
                        type="button"
                        role="radio"
                        aria-checked={active}
                        onClick={() => setExecutionMode(mode)}
                        className={
                          'flex-1 px-md py-xs rounded-lg border font-label-md capitalize cursor-pointer transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 ' +
                          (active
                            ? 'bg-primary text-on-primary border-primary'
                            : 'border-outline-variant/50 text-on-surface-variant hover:bg-surface-container')
                        }
                      >
                        {mode}
                      </button>
                    )
                  })}
                </div>
              ) : (
                <p className="font-body-md text-on-surface mt-xs capitalize">
                  {task.execution_mode ?? 'serial'}
                </p>
              )}
            </div>
          )}

          {/* Dependencies (read-only — editing happens via DependsOnEditor) */}
          {editable && (task.blocked_by?.length || task.blocks?.length) ? (
            <div>
              <span className="text-label-sm text-on-surface-variant">{t('tasks.taskDetailDrawer.dependencies')}</span>
              <div className="mt-xs flex flex-col gap-xs">
                {task.blocked_by?.length ? (
                  <div className="text-body-sm text-on-surface-variant">
                    {t('tasks.taskDetailDrawer.blockedBy')}{' '}
                    <span className="text-on-surface">{task.blocked_by.join(', ')}</span>
                  </div>
                ) : null}
                {task.blocks?.length ? (
                  <div className="text-body-sm text-on-surface-variant">
                    {t('tasks.taskDetailDrawer.blocks')}{' '}
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
                      setExecutionMode(task.execution_mode === 'parallel' ? 'parallel' : 'serial')
                    }}
                    disabled={saving}
                    className="px-md py-xs rounded-lg text-on-surface-variant font-label-md hover:bg-surface-container cursor-pointer focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 disabled:opacity-50"
                  >
                    {t('tasks.taskDetailDrawer.cancel')}
                  </button>
                  <button
                    onClick={() => void handleSave()}
                    disabled={saving}
                    className="px-md py-xs rounded-lg bg-primary text-on-primary font-label-md hover:brightness-110 transition-all cursor-pointer focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 disabled:opacity-50"
                  >
                    {saving ? t('tasks.taskDetailDrawer.saving') : t('tasks.taskDetailDrawer.save')}
                  </button>
                </>
              ) : (
                <button
                  onClick={() => setEditing(true)}
                  className="px-md py-xs rounded-lg bg-primary/10 text-primary font-label-md hover:bg-primary/20 transition-colors cursor-pointer focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
                >
                  <span className="material-symbols-outlined text-[16px] align-middle mr-xs">edit</span>
                  {t('tasks.taskDetailDrawer.edit')}
                </button>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  )
}
