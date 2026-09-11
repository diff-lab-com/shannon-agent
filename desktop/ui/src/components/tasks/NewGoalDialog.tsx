import { useState } from 'react'
import { useIntl } from 'react-intl'
import { Modal, ModalBody } from '@/components/ui/modal'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { useT } from '@/i18n'

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
