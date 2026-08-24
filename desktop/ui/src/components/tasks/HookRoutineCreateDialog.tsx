// HookRoutineCreateDialog — modal form for creating a triggered routine.
//
// Persists new routines to `.shannon/routines.toml` via the
// `create_triggered_routine` Tauri command. On success, callers should refresh
// their routine list (the dialog calls onCreated with the new routine DTO).
//
// T1.2 — migrated onto the shared <Modal> primitive; the previous
// hand-rolled overlay, focus containerRef, and backdrop click handler
// are gone. Modal owns focus trap, Escape close, scroll lock, and
// backdrop dismiss.

import { useState, useEffect } from 'react'
import { useIntl } from 'react-intl'
import { Modal, ModalBody, ModalFooter } from '@/components/ui/modal'
import { Button } from '@/components/ui/button'
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
    <Modal
      open={open}
      onClose={onClose}
      size="lg"
      // Modal renders its own header (with close button) when title is set.
      // closeLabel supplies the close button's aria-label = "Close dialog"
      // (test contract in HookRoutineCreateDialog.test.tsx). The built-in
      // header h2 satisfies the "role=dialog" + aria-labelledby semantic
      // and gives Modal the close button so the backdrop-click + Esc
      // primitive behavior is consistent with the rest of the dialogs.
      title={t('tasks.hookRoutineCreateDialog.title')}
      closeLabel={t('tasks.hookRoutineCreateDialog.closeAria')}
      busy={submitting}
      className="max-h-[90vh] flex flex-col"
    >
      <form onSubmit={onSubmit} className="flex flex-col gap-md">
        <ModalBody className="pt-0 space-y-md">
          {error ? (
            <div className="bg-error/10 border border-error/30 rounded-lg p-sm font-label-sm text-error flex items-start gap-sm" role="alert">
              <span className="material-symbols-outlined text-[14px] mt-0.5" aria-hidden="true">error</span>
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
        </ModalBody>

        <ModalFooter className="pt-sm border-t border-outline-variant/20">
          <Button
            type="button"
            variant="ghost"
            onClick={onClose}
            disabled={submitting}
          >
            {t('tasks.hookRoutineCreateDialog.cancel')}
          </Button>
          <Button
            type="submit"
            disabled={!canSubmit}
            className="disabled:cursor-not-allowed"
          >
            <span className="material-symbols-outlined text-[14px]" aria-hidden="true">{submitting ? 'hourglass_top' : 'add'}</span>
            {submitting ? t('tasks.hookRoutineCreateDialog.creating') : t('tasks.hookRoutineCreateDialog.createRoutine')}
          </Button>
        </ModalFooter>
      </form>
    </Modal>
  )
}