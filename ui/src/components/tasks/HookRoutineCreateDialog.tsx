// HookRoutineCreateDialog — modal form for creating a triggered routine.
//
// Persists new routines to `.shannon/routines.toml` via the
// `create_triggered_routine` Tauri command. On success, callers should refresh
// their routine list (the dialog calls onCreated with the new routine DTO).

import { useState, useEffect, useRef } from 'react'
import { useIntl } from 'react-intl'
import { useModalFocus } from '@/hooks/useModalFocus'
import * as api from '@/lib/tauri-api'
import type { TriggeredRoutineDto } from '@/types'

const TRIGGER_OPTIONS: Array<{ value: string; label: string; hint: string }> = [
  { value: 'PostToolUse', label: 'PostToolUse', hint: 'After any tool runs (e.g. after edit, bash)' },
  { value: 'PreToolUse', label: 'PreToolUse', hint: 'Before a tool runs (gated approval)' },
  { value: 'TaskCompleted', label: 'TaskCompleted', hint: 'When a task is marked completed' },
  { value: 'TaskCreated', label: 'TaskCreated', hint: 'When a new task is created' },
  { value: 'SubagentStart', label: 'SubagentStart', hint: 'When a subagent launches' },
  { value: 'SubagentStop', label: 'SubagentStop', hint: 'When a subagent finishes' },
  { value: 'PreCompact', label: 'PreCompact', hint: 'Before context compaction' },
  { value: 'PostCompact', label: 'PostCompact', hint: 'After context compaction' },
  { value: 'ConfigChange', label: 'ConfigChange', hint: 'When shannon config changes' },
]

export interface HookRoutineCreateDialogProps {
  open: boolean
  onClose: () => void
  onCreated: (routine: TriggeredRoutineDto) => void
}

export default function HookRoutineCreateDialog({ open, onClose, onCreated }: HookRoutineCreateDialogProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const [name, setName] = useState('')
  const [trigger, setTrigger] = useState('PostToolUse')
  const [command, setCommand] = useState('')
  const [matcher, setMatcher] = useState('')
  const [pattern, setPattern] = useState('')
  const [description, setDescription] = useState('')
  const [submitting, setSubmitting] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const containerRef = useRef<HTMLDivElement>(null)
  useModalFocus(open, containerRef)

  useEffect(() => {
    if (open) {
      setName('')
      setTrigger('PostToolUse')
      setCommand('')
      setMatcher('')
      setPattern('')
      setDescription('')
      setSubmitting(false)
      setError(null)
    }
  }, [open])

  if (!open) return null

  const nameOk = name.trim().length >= 1 && /^[a-zA-Z][a-zA-Z0-9_-]*$/.test(name.trim())
  const commandOk = command.trim().length > 0
  const canSubmit = nameOk && commandOk && !submitting

  const onSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!canSubmit) return
    setSubmitting(true)
    setError(null)
    try {
      const created = await api.createTriggeredRoutine({
        name: name.trim(),
        trigger,
        command: command.trim(),
        matcher: matcher.trim() || undefined,
        pattern: pattern.trim() || undefined,
        description: description.trim() || undefined,
      })
      onCreated(created)
      onClose()
    } catch (err) {
      setError(String(err))
    } finally {
      setSubmitting(false)
    }
  }

  const selectedHint = TRIGGER_OPTIONS.find(o => o.value === trigger)?.hint ?? ''

  return (
    <div
      ref={containerRef}
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-md"
      role="dialog"
      aria-modal="true"
      aria-labelledby="hook-routine-create-title"
      onClick={onClose}
    >
      <form
        onClick={(e) => e.stopPropagation()}
        onSubmit={onSubmit}
        className="bg-surface-container-lowest rounded-2xl shadow-2xl border border-outline-variant/40 w-full max-w-lg max-h-[90vh] overflow-y-auto p-lg flex flex-col gap-md"
      >
        <div className="flex items-center justify-between">
          <h2 id="hook-routine-create-title" className="font-headline-md text-[18px] font-bold text-on-surface flex items-center gap-sm">
            <span className="material-symbols-outlined icon-md text-primary">add_link</span>
            {t('tasks.hookRoutineCreateDialog.title')}
          </h2>
          <button
            type="button"
            onClick={onClose}
            aria-label={t('tasks.hookRoutineCreateDialog.closeAria')}
            className="text-on-surface-variant hover:bg-surface-container-high rounded-full p-xs cursor-pointer focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
          >
            <span className="material-symbols-outlined text-[18px]">close</span>
          </button>
        </div>

        {error ? (
          <div className="bg-error/10 border border-error/30 rounded-lg p-sm font-label-sm text-error flex items-start gap-sm" role="alert">
            <span className="material-symbols-outlined text-[14px] mt-0.5">error</span>
            <span className="flex-1 break-words">{error}</span>
          </div>
        ) : null}

        <label className="flex flex-col gap-xs">
          <span className="font-label-md text-on-surface">{t('tasks.hookRoutineCreateDialog.name')}</span>
          <input
            type="text"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder={t('tasks.hookRoutineCreateDialog.namePlaceholder')}
            required
            aria-invalid={!nameOk && name.length > 0}
            className="bg-surface-container-low border border-outline-variant/40 rounded-lg px-md py-sm font-body-md text-on-surface focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/40"
          />
          <span className="font-label-sm text-[11px] text-on-surface-variant">
            {t('tasks.hookRoutineCreateDialog.nameHint')}
          </span>
        </label>

        <label className="flex flex-col gap-xs">
          <span className="font-label-md text-on-surface">{t('tasks.hookRoutineCreateDialog.hookEvent')}</span>
          <select
            value={trigger}
            onChange={(e) => setTrigger(e.target.value)}
            className="bg-surface-container-low border border-outline-variant/40 rounded-lg px-md py-sm font-body-md text-on-surface focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/40"
          >
            {TRIGGER_OPTIONS.map(o => (
              <option key={o.value} value={o.value}>{o.label}</option>
            ))}
          </select>
          <span className="font-label-sm text-[11px] text-on-surface-variant">{selectedHint}</span>
        </label>

        <label className="flex flex-col gap-xs">
          <span className="font-label-md text-on-surface">{t('tasks.hookRoutineCreateDialog.command')}</span>
          <input
            type="text"
            value={command}
            onChange={(e) => setCommand(e.target.value)}
            placeholder={t('tasks.hookRoutineCreateDialog.commandPlaceholder')}
            required
            className="bg-surface-container-low border border-outline-variant/40 rounded-lg px-md py-sm font-body-md font-mono text-on-surface focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/40"
          />
          <span className="font-label-sm text-[11px] text-on-surface-variant">
            {t('tasks.hookRoutineCreateDialog.commandHint')}
          </span>
        </label>

        <div className="grid grid-cols-2 gap-md">
          <label className="flex flex-col gap-xs">
            <span className="font-label-md text-on-surface">{t('tasks.hookRoutineCreateDialog.matcher')}</span>
            <input
              type="text"
              value={matcher}
              onChange={(e) => setMatcher(e.target.value)}
              placeholder={t('tasks.hookRoutineCreateDialog.matcherPlaceholder')}
              className="bg-surface-container-low border border-outline-variant/40 rounded-lg px-md py-sm font-body-md font-mono text-on-surface focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/40"
            />
          </label>
          <label className="flex flex-col gap-xs">
            <span className="font-label-md text-on-surface">{t('tasks.hookRoutineCreateDialog.pattern')}</span>
            <input
              type="text"
              value={pattern}
              onChange={(e) => setPattern(e.target.value)}
              placeholder={t('tasks.hookRoutineCreateDialog.patternPlaceholder')}
              className="bg-surface-container-low border border-outline-variant/40 rounded-lg px-md py-sm font-body-md font-mono text-on-surface focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/40"
            />
          </label>
        </div>

        <label className="flex flex-col gap-xs">
          <span className="font-label-md text-on-surface">{t('tasks.hookRoutineCreateDialog.description')}</span>
          <textarea
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            placeholder={t('tasks.hookRoutineCreateDialog.descriptionPlaceholder')}
            rows={2}
            className="bg-surface-container-low border border-outline-variant/40 rounded-lg px-md py-sm font-body-md text-on-surface focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/40 resize-none"
          />
        </label>

        <div className="flex justify-end gap-sm pt-sm border-t border-outline-variant/20">
          <button
            type="button"
            onClick={onClose}
            disabled={submitting}
            className="px-md py-sm rounded-lg font-label-md text-on-surface-variant hover:bg-surface-container-high cursor-pointer focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 disabled:opacity-40"
          >
            {t('tasks.hookRoutineCreateDialog.cancel')}
          </button>
          <button
            type="submit"
            disabled={!canSubmit}
            className="px-md py-sm rounded-lg font-label-md text-on-primary bg-primary hover:bg-primary/90 cursor-pointer focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/40 disabled:opacity-40 disabled:cursor-not-allowed flex items-center gap-1"
          >
            <span className="material-symbols-outlined text-[14px]">{submitting ? 'hourglass_top' : 'add'}</span>
            {submitting ? t('tasks.hookRoutineCreateDialog.creating') : t('tasks.hookRoutineCreateDialog.createRoutine')}
          </button>
        </div>
      </form>
    </div>
  )
}
