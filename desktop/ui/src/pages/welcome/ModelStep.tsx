// Step 1 — model / provider. P1.2-C: launches the canonical AddProviderModal
// instead of authoring a bespoke provider picker + bare API-key input.
// Extracted from Welcome.tsx (T3.1).
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import { WelcomeCard } from './components'
import { PROVIDERS, TASKS, type TaskId } from './constants'

interface ModelStepProps {
  task: TaskId
  saving: boolean
  canContinue: boolean
  onOpenAddProvider: () => void
  onBack: () => void
  onContinue: () => void
}

export function ModelStep({ task, saving, canContinue, onOpenAddProvider, onBack, onContinue }: ModelStepProps) {
  const intl = useIntl()
  const currentTask = TASKS.find(t => t.id === task)!
  const recommendedProvider = PROVIDERS.find(p => p.id === currentTask.recommendedProvider)
  return (
    <WelcomeCard
      title={intl.formatMessage({ id: 'welcome.model.title' })}
      subtitle={
        recommendedProvider
          ? intl.formatMessage(
              { id: 'welcome.model.subtitle.recommended' },
              { task: intl.formatMessage({ id: currentTask.labelKey }), provider: recommendedProvider.label },
            )
          : intl.formatMessage({ id: 'welcome.model.subtitle.default' })
      }
      footer={
        <>
          <Button variant="ghost" onClick={onBack} className="px-lg py-sm text-on-surface-variant hover:text-primary font-label-md cursor-pointer focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary rounded">
            {intl.formatMessage({ id: 'welcome.model.back' })}
          </Button>
          <div className="flex flex-col items-end gap-xs">
            <Button
              onClick={onContinue}
              disabled={saving || !canContinue}
              className="px-lg py-sm bg-primary text-on-primary rounded-lg font-label-md cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed hover:bg-primary/90 transition-colors focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary"
            >
              {intl.formatMessage({ id: 'welcome.model.continue' })}
            </Button>
            {!canContinue ? (
              <span className="font-label-sm text-on-surface-variant">
                {intl.formatMessage({ id: 'welcome.model.continue.hint' })}
              </span>
            ) : null}
          </div>
        </>
      }
    >
      <Button
        type="button"
        variant="outline"
        onClick={onOpenAddProvider}
        className="w-full p-md rounded-xl border border-outline-variant/50 hover:border-primary/50 bg-surface-container-low cursor-pointer transition-all focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary flex items-center gap-md"
        data-testid="welcome-add-provider"
      >
        <span className="material-symbols-outlined text-primary">add_circle</span>
        <span className="flex-1 text-left">
          <div className="font-headline-md text-on-surface">
            {intl.formatMessage({ id: 'welcome.model.addProvider.label' })}
          </div>
          <div className="font-body-sm text-on-surface-variant mt-xs">
            {intl.formatMessage({ id: 'welcome.model.addProvider.help' })}
          </div>
        </span>
        <span className="material-symbols-outlined text-on-surface-variant">arrow_forward</span>
      </Button>
      <p className="font-body-sm text-on-surface-variant mt-md">
        {intl.formatMessage({ id: 'welcome.model.addProvider.testHint' })}
      </p>
    </WelcomeCard>
  )
}