import { useState } from 'react'
import { useIntl } from 'react-intl'
import { Modal, ModalBody } from '@/components/ui/modal'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { useT } from '@/i18n'

// Audit §13 — Goal templates mirror the Welcome task cards (Code / Writing
// / Research / General) but framed as long-running objectives. The objective
// text is what `start_goal_run` passes to the engine, so these are real
// working prompts, not placeholders.
const GOAL_TEMPLATES = [
  { id: 'code', icon: 'code', labelKey: 'goal.new.template.code.label', descKey: 'goal.new.template.code.desc', titleKey: 'goal.new.template.code.title', objectiveKey: 'goal.new.template.code.objective' },
  { id: 'research', icon: 'search', labelKey: 'goal.new.template.research.label', descKey: 'goal.new.template.research.desc', titleKey: 'goal.new.template.research.title', objectiveKey: 'goal.new.template.research.objective' },
  { id: 'refactor', icon: 'auto_fix', labelKey: 'goal.new.template.refactor.label', descKey: 'goal.new.template.refactor.desc', titleKey: 'goal.new.template.refactor.title', objectiveKey: 'goal.new.template.refactor.objective' },
] as const

interface NewGoalDialogProps {
  open: boolean
  onClose: () => void
  onStart: (input: { title: string; objective: string; maxTurns?: number; budgetUsd?: number }) => Promise<unknown>
}

/**
 * Create-a-goal dialog (audit C11 — the desktop goal runner had list/stop/
 * pause surfaces but no creation entry; `startGoalRun` was unreachable from
 * the UI). Budget/turn caps are optional and map to the CLI goal system's
 * budget-cap / max-turns contract.
 */
export default function NewGoalDialog({ open, onClose, onStart }: NewGoalDialogProps) {
  const intl = useIntl()
  const t = useT()
  const [title, setTitle] = useState('')
  const [objective, setObjective] = useState('')
  const [maxTurns, setMaxTurns] = useState('')
  const [budgetUsd, setBudgetUsd] = useState('')
  const [submitting, setSubmitting] = useState(false)

  const valid = title.trim().length > 0 && objective.trim().length > 0

  const reset = () => {
    setTitle(''); setObjective(''); setMaxTurns(''); setBudgetUsd('')
  }

  const handleSubmit = async () => {
    if (!valid || submitting) return
    setSubmitting(true)
    try {
      await onStart({
        title: title.trim(),
        objective: objective.trim(),
        maxTurns: maxTurns ? Number(maxTurns) : undefined,
        budgetUsd: budgetUsd ? Number(budgetUsd) : undefined,
      })
      reset()
      onClose()
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <Modal open={open} onClose={onClose} title={t('goal.new.title')} size="md">
      <ModalBody>
        <p className="font-body-sm text-on-surface-variant mb-md">{t('goal.new.description')}</p>

        {/* Templates — click to fill in the form. Cards stay compact so the
            form below remains the primary reading order. */}
        <div className="grid grid-cols-3 gap-sm mb-lg">
          {GOAL_TEMPLATES.map(tpl => (
            <button
              key={tpl.id}
              type="button"
              onClick={() => {
                setTitle(intl.formatMessage({ id: tpl.titleKey }))
                setObjective(intl.formatMessage({ id: tpl.objectiveKey }))
              }}
              className="text-left p-sm rounded-xl border border-outline-variant/40 bg-surface-container-lowest hover:border-primary hover:bg-primary/5 transition-colors focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
              data-testid={`goal-template-${tpl.id}`}
            >
              <span className="material-symbols-outlined text-primary text-[18px] mb-1 block" aria-hidden="true">{tpl.icon}</span>
              <span className="font-label-sm text-on-surface font-bold block">{intl.formatMessage({ id: tpl.labelKey })}</span>
              <span className="font-body-xs text-on-surface-variant line-clamp-2">{intl.formatMessage({ id: tpl.descKey })}</span>
            </button>
          ))}
        </div>

        <div className="space-y-md">
          <label className="block">
            <span className="font-label-md text-on-surface mb-xs block">{t('goal.new.label.title')}</span>
            <Input
              value={title}
              onChange={e => setTitle(e.target.value)}
              placeholder={intl.formatMessage({ id: 'goal.new.placeholder.title' })}
              maxLength={120}
              autoFocus
            />
          </label>
          <label className="block">
            <span className="font-label-md text-on-surface mb-xs block">{t('goal.new.label.objective')}</span>
            <textarea
              value={objective}
              onChange={e => setObjective(e.target.value)}
              placeholder={intl.formatMessage({ id: 'goal.new.placeholder.objective' })}
              rows={4}
              className="w-full rounded-lg border border-outline-variant/50 bg-surface-container-lowest px-sm py-xs font-body-md text-on-surface placeholder:text-on-surface-variant/60 focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
            />
          </label>
          <div className="grid grid-cols-2 gap-md">
            <label className="block">
              <span className="font-label-md text-on-surface mb-xs block">{t('goal.new.label.maxTurns')}</span>
              <Input
                type="number"
                min={1}
                value={maxTurns}
                onChange={e => setMaxTurns(e.target.value)}
                placeholder={intl.formatMessage({ id: 'goal.new.placeholder.maxTurns' })}
              />
            </label>
            <label className="block">
              <span className="font-label-md text-on-surface mb-xs block">{t('goal.new.label.budget')}</span>
              <Input
                type="number"
                min={0}
                step="0.5"
                value={budgetUsd}
                onChange={e => setBudgetUsd(e.target.value)}
                placeholder={intl.formatMessage({ id: 'goal.new.placeholder.budget' })}
              />
            </label>
          </div>
        </div>
        <div className="flex justify-end gap-sm mt-lg">
          <Button variant="ghost" onClick={onClose} className="cursor-pointer">
            {t('goal.new.cancel')}
          </Button>
          <Button
            onClick={() => void handleSubmit()}
            disabled={!valid || submitting}
            className="cursor-pointer bg-primary text-on-primary hover:bg-primary/90 disabled:opacity-50"
          >
            {submitting ? t('goal.new.submitting') : t('goal.new.submit')}
          </Button>
        </div>
      </ModalBody>
    </Modal>
  )
}
