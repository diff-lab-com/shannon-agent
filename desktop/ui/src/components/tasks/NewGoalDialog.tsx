import { useState } from 'react'
import { useForm } from 'react-hook-form'
import { zodResolver } from '@hookform/resolvers/zod'
import { z } from 'zod'
import { useIntl } from 'react-intl'
import { Modal, ModalBody } from '@/components/ui/modal'
import { Button } from '@/components/ui/button'
import { Form, FormField, FormInput } from '@/components/ui/form'
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
const goalSchema = z.object({
  title: z.string().trim().min(1).max(120),
  objective: z.string().trim().min(1),
  maxTurns: z.coerce.number().int().min(1).optional(),
  budgetUsd: z.coerce.number().min(0).optional(),
})
type GoalFormValues = z.input<typeof goalSchema>

export default function NewGoalDialog({ open, onClose, onStart }: NewGoalDialogProps) {
  const intl = useIntl()
  const t = useT()
  const [submitting, setSubmitting] = useState(false)
  const form = useForm<GoalFormValues>({
    resolver: zodResolver(goalSchema),
    defaultValues: { title: '', objective: '', maxTurns: '', budgetUsd: '' },
  })

  const handleSubmit = form.handleSubmit(async values => {
    setSubmitting(true)
    try {
      await onStart({
        title: values.title,
        objective: values.objective,
        maxTurns: values.maxTurns != null ? Number(values.maxTurns) : undefined,
        budgetUsd: values.budgetUsd != null ? Number(values.budgetUsd) : undefined,
      })
      form.reset()
      onClose()
    } finally {
      setSubmitting(false)
    }
  })

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
                // Templates fill both fields; mark them valid by clearing errors.
                form.setValue('title', intl.formatMessage({ id: tpl.titleKey }), { shouldValidate: true })
                form.setValue('objective', intl.formatMessage({ id: tpl.objectiveKey }), { shouldValidate: true })
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

        <Form id="new-goal-form" onSubmit={handleSubmit} className="space-y-md mt-0">
          <FormField
            name="title"
            label={t('goal.new.label.title')}
            required
            error={form.formState.errors.title ? t('goal.new.error.title') : undefined}
          >
            <FormInput
              id="title"
              {...form.register('title')}
              placeholder={intl.formatMessage({ id: 'goal.new.placeholder.title' })}
              maxLength={120}
              autoFocus
              invalid={!!form.formState.errors.title}
            />
          </FormField>
          <FormField
            name="objective"
            label={t('goal.new.label.objective')}
            required
            error={form.formState.errors.objective ? t('goal.new.error.objective') : undefined}
          >
            <textarea
              id="objective"
              {...form.register('objective')}
              placeholder={intl.formatMessage({ id: 'goal.new.placeholder.objective' })}
              rows={4}
              aria-invalid={!!form.formState.errors.objective || undefined}
              className="w-full rounded-lg border border-outline-variant/50 bg-surface-container-lowest px-sm py-xs font-body-md text-on-surface placeholder:text-on-surface-variant/60 focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary aria-[invalid=true]:border-error"
            />
          </FormField>
          <div className="grid grid-cols-2 gap-md">
            <FormField
              name="maxTurns"
              label={t('goal.new.label.maxTurns')}
              error={form.formState.errors.maxTurns ? t('goal.new.error.maxTurns') : undefined}
            >
              <FormInput
                id="maxTurns"
                type="number"
                min={1}
                {...form.register('maxTurns')}
                placeholder={intl.formatMessage({ id: 'goal.new.placeholder.maxTurns' })}
                invalid={!!form.formState.errors.maxTurns}
              />
            </FormField>
            <FormField
              name="budgetUsd"
              label={t('goal.new.label.budget')}
              error={form.formState.errors.budgetUsd ? t('goal.new.error.budget') : undefined}
            >
              <FormInput
                id="budgetUsd"
                type="number"
                min={0}
                step="0.5"
                {...form.register('budgetUsd')}
                placeholder={intl.formatMessage({ id: 'goal.new.placeholder.budget' })}
                invalid={!!form.formState.errors.budgetUsd}
              />
            </FormField>
          </div>
        </Form>
        <div className="flex justify-end gap-sm mt-lg">
          <Button variant="ghost" onClick={onClose} className="cursor-pointer">
            {t('goal.new.cancel')}
          </Button>
          <Button
            type="submit"
            form="new-goal-form"
            disabled={submitting}
            className="cursor-pointer bg-primary text-on-primary hover:bg-primary/90 disabled:opacity-50"
          >
            {submitting ? t('goal.new.submitting') : t('goal.new.submit')}
          </Button>
        </div>
      </ModalBody>
    </Modal>
  )
}
