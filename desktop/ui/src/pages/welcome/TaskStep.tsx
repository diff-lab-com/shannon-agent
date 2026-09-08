// Step 0 — pick the primary use case. Drives the recommended provider +
// tool defaults for later steps. Extracted from Welcome.tsx (T3.1).
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { WelcomeCard } from './components'
import { TASKS, type TaskId } from './constants'

interface TaskStepProps {
  // null until the user picks a card — Continue stays disabled.
  task: TaskId | null
  setTask: (id: TaskId) => void
  onContinue: () => void
}

export function TaskStep({ task, setTask, onContinue }: TaskStepProps) {
  const intl = useIntl()
  return (
    <WelcomeCard
      title={intl.formatMessage({ id: 'welcome.task.title' })}
      subtitle={intl.formatMessage({ id: 'welcome.task.subtitle' })}
      footer={
        <>
          <span />
          <Button
            onClick={onContinue}
            disabled={task === null}
            className="px-lg py-sm bg-primary text-on-primary rounded-lg font-label-md cursor-pointer hover:bg-primary/90 transition-colors focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary"
          >
            {intl.formatMessage({ id: 'welcome.task.continue' })}
          </Button>
        </>
      }
    >
      <div className="grid grid-cols-1 sm:grid-cols-2 gap-sm mb-lg">
        {TASKS.map(t => (
          <Button
            key={t.id}
            variant="outline"
            onClick={() => setTask(t.id)}
            aria-pressed={task === t.id}
            className={cn(
              'h-auto text-left p-md rounded-xl border cursor-pointer transition-all focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary whitespace-normal',
              task === t.id
                ? 'border-2 border-primary bg-primary-container/5'
                : 'border-outline-variant/50 hover:border-primary/50',
            )}
          >
            <div className="flex items-start gap-sm">
              <span className="material-symbols-outlined text-primary shrink-0">{t.icon}</span>
              <div className="flex-1">
                <div className="font-headline-md text-on-surface">{intl.formatMessage({ id: t.labelKey })}</div>
                <div className="font-body-sm text-on-surface-variant mt-xs">{intl.formatMessage({ id: t.blurbKey })}</div>
              </div>
              <div className={cn('w-5 h-5 rounded-full border-2 shrink-0', task === t.id ? 'border-primary bg-primary' : 'border-outline-variant')} />
            </div>
          </Button>
        ))}
      </div>
    </WelcomeCard>
  )
}