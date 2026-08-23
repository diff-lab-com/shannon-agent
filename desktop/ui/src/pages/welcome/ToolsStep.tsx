// Step 2 — tool toggles. Extracted from Welcome.tsx (T3.1).
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import { WelcomeCard } from './components'
import { TOOL_CATALOG, TASKS, type TaskId } from './constants'

interface ToolsStepProps {
  task: TaskId
  enabledTools: Record<string, boolean>
  toggleTool: (id: string) => void
  onBack: () => void
  onContinue: () => void
  onOpenSettings: () => void
}

export function ToolsStep({ task, enabledTools, toggleTool, onBack, onContinue, onOpenSettings }: ToolsStepProps) {
  const intl = useIntl()
  const currentTask = TASKS.find(t => t.id === task)!
  return (
    <WelcomeCard
      title={intl.formatMessage({ id: 'welcome.tools.title' })}
      subtitle={intl.formatMessage({ id: 'welcome.tools.subtitle' })}
      footer={
        <>
          <Button variant="ghost" onClick={onBack} className="px-lg py-sm text-on-surface-variant hover:text-primary font-label-md cursor-pointer focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary rounded">
            {intl.formatMessage({ id: 'welcome.model.back' })}
          </Button>
          <Button
            onClick={onContinue}
            className="px-lg py-sm bg-primary text-on-primary rounded-lg font-label-md cursor-pointer hover:bg-primary/90 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary"
          >
            {intl.formatMessage({ id: 'welcome.tools.continue' })}
          </Button>
        </>
      }
    >
      <div className="space-y-sm mb-lg">
        {Object.entries(TOOL_CATALOG).map(([id, meta]) => {
          const enabled = !!enabledTools[id]
          const recommended = currentTask.tools.includes(id)
          const toolLabel = intl.formatMessage({ id: meta.labelKey })
          return (
            <label
              key={id}
              className={`flex items-start gap-md p-md rounded-xl border cursor-pointer transition-all ${
                enabled ? 'border-2 border-primary bg-primary-container/5' : 'border-outline-variant/50 hover:border-primary/50'
              }`}
            >
              <input
                type="checkbox"
                checked={enabled}
                onChange={() => toggleTool(id)}
                className="mt-xs accent-primary"
                aria-label={intl.formatMessage({ id: 'welcome.tools.enableAria' }, { label: toolLabel })}
              />
              <span className="material-symbols-outlined text-on-surface-variant shrink-0">{meta.icon}</span>
              <div className="flex-1">
                <div className="flex items-center gap-xs">
                  <span className="font-headline-md text-on-surface">{toolLabel}</span>
                  {recommended && (
                    <span className="text-[10px] uppercase tracking-wider font-bold text-primary bg-primary/10 px-1.5 py-0.5 rounded">
                      {intl.formatMessage({ id: 'welcome.tools.recommended' })}
                    </span>
                  )}
                </div>
                <div className="font-body-sm text-on-surface-variant mt-xs">{intl.formatMessage({ id: meta.descKey })}</div>
              </div>
            </label>
          )
        })}
      </div>
      <p className="font-body-sm text-on-surface-variant">
        {intl.formatMessage(
          { id: 'welcome.tools.workingDir.help' },
          {
            link: (chunks: React.ReactNode) => (
              <Button
                variant="link"
                onClick={onOpenSettings}
                className="text-primary hover:underline cursor-pointer p-0 h-auto"
              >
                {chunks}
              </Button>
            ),
          },
        )}
      </p>
    </WelcomeCard>
  )
}