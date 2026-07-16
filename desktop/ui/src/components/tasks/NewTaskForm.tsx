// Inline form for creating a new background task (prompt-based, fires immediately).
//
// C2 (Phase D): optional assignee + priority fields. When present, they are
// embedded into the prompt as `[Assignee: X][Priority: high] ...` since the
// backend startBackgroundTask signature only accepts a prompt string. The
// composePrompt helper centralizes this format.
//
// Preserves the original monolith's behavior: Enter submits (Shift+Enter for
// newline), char counter, Create Task disabled when empty.

import { useState } from 'react'
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'

export type Priority = 'low' | 'medium' | 'high'

export function composePrompt(prompt: string, assignee?: string, priority?: Priority): string {
  const tags: string[] = []
  if (assignee?.trim()) tags.push(`[Assignee: ${assignee.trim()}]`)
  if (priority && priority !== 'low') tags.push(`[Priority: ${priority}]`)
  return tags.length > 0 ? `${tags.join(' ')} ${prompt}` : prompt
}

interface NewTaskFormProps {
  value: string
  onChange: (value: string) => void
  onSubmit: (rich: { prompt: string; assignee: string; priority: Priority }) => void
  onCancel: () => void
}

export default function NewTaskForm({ value, onChange, onSubmit, onCancel }: NewTaskFormProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  const [assignee, setAssignee] = useState('')
  const [priority, setPriority] = useState<Priority>('low')
  const [showMeta, setShowMeta] = useState(false)

  const submit = () => {
    if (!value.trim()) return
    onSubmit({ prompt: composePrompt(value, assignee, priority), assignee: assignee.trim(), priority })
  }

  return (
    <div className="bg-surface-container-lowest border border-primary/30 rounded-xl p-lg mb-lg flex flex-col gap-md shadow-sm">
      <div className="flex items-center justify-between">
        <h3 className="font-body-lg font-bold text-on-surface">{t('tasks.newTaskForm.title')}</h3>
        <button
          type="button"
          className="font-label-sm text-primary hover:bg-primary/10 rounded px-sm py-xs cursor-pointer flex items-center gap-1 focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
          onClick={() => setShowMeta(!showMeta)}
          aria-expanded={showMeta}
          aria-controls="new-task-meta"
        >
          <span className="material-symbols-outlined text-[14px]">{showMeta ? 'remove' : 'add'}</span>
          {showMeta ? t('tasks.newTaskForm.hideOptions') : t('tasks.newTaskForm.addOptions')}
        </button>
      </div>
      <textarea
        className={`w-full h-20 p-sm bg-surface-container-low rounded-lg border text-body-sm resize-none focus:outline-none focus:ring-2 focus:ring-primary/30 ${!value.trim() ? 'border-outline-variant/30' : 'border-primary/30'}`}
        placeholder={t('tasks.newTaskForm.placeholder')}
        value={value}
        onChange={e => onChange(e.target.value)}
        onKeyDown={e => { if (e.key === 'Enter' && !e.shiftKey && value.trim()) { e.preventDefault(); submit() } }}
        autoFocus
      />
      {showMeta ? (
        <div id="new-task-meta" className="grid grid-cols-1 md:grid-cols-2 gap-md">
          <label className="flex flex-col gap-xs">
            <span className="font-label-md text-on-surface-variant">{t('tasks.newTaskForm.assigneeLabel')}</span>
            <input
              type="text"
              placeholder={t('tasks.newTaskForm.assigneePlaceholder')}
              value={assignee}
              onChange={e => setAssignee(e.target.value)}
              className="bg-surface-container-low rounded-lg border border-outline-variant/30 px-sm py-sm text-body-sm focus:outline-none focus:ring-2 focus:ring-primary/30"
            />
          </label>
          <label className="flex flex-col gap-xs">
            <span className="font-label-md text-on-surface-variant">{t('tasks.newTaskForm.priority')}</span>
            <select
              value={priority}
              onChange={e => setPriority(e.target.value as Priority)}
              className="bg-surface-container-low rounded-lg border border-outline-variant/30 px-sm py-sm text-body-sm focus:outline-none focus:ring-2 focus:ring-primary/30 cursor-pointer"
            >
              <option value="low">{t('tasks.newTaskForm.low')}</option>
              <option value="medium">{t('tasks.newTaskForm.medium')}</option>
              <option value="high">{t('tasks.newTaskForm.high')}</option>
            </select>
          </label>
        </div>
      ) : null}
      <div className="flex items-center justify-between">
        <span className="font-label-sm text-on-surface-variant">{value.length > 0 ? intl.formatMessage({ id: 'tasks.newTaskForm.chars' }, { count: value.length }) : ''}</span>
        <div className="flex gap-sm">
          <Button
            className="px-md py-sm bg-primary text-on-primary rounded-lg font-label-md cursor-pointer disabled:opacity-50"
            onClick={submit}
            disabled={!value.trim()}
          >
            {t('tasks.newTaskForm.createTask')}
          </Button>
          <Button
            variant="ghost"
            className="px-md py-sm rounded-lg border border-outline-variant font-label-md cursor-pointer"
            onClick={() => { setAssignee(''); setPriority('low'); setShowMeta(false); onCancel() }}
          >
            {t('tasks.newTaskForm.cancel')}
          </Button>
        </div>
      </div>
    </div>
  )
}
