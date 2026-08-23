// Step 3 — final summary + workspace picker + shortcuts + dev mode opt-in
// + optional Documents skills list. Extracted from Welcome.tsx (T3.1).
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import { WelcomeCard } from './components'
import { DOCUMENTS_SKILLS_AVAILABLE, PROVIDERS, SHORTCUT_ROWS, TASKS, type TaskId, type DocumentsSkill } from './constants'
import { DocumentsSkillsList } from './DocumentsSkillsList'

interface SkillState {
  status: 'idle' | 'installing' | 'installed' | 'failed'
  error?: string
}

interface DoneStepProps {
  task: TaskId
  provider: string
  enabledToolCount: number
  pickedDir: string | null
  fallbackWorkingDir: string | null
  devMode: boolean
  setDevMode: React.Dispatch<React.SetStateAction<boolean>>
  skillState: Record<string, SkillState>
  onPickDirectory: () => void
  onBack: () => void
  onFinish: () => void
  onInstallSkill: (skill: DocumentsSkill) => void
  onBrowseFeaturedSkills: () => void
}

export function DoneStep({
  task,
  provider,
  enabledToolCount,
  pickedDir,
  fallbackWorkingDir,
  devMode,
  setDevMode,
  skillState,
  onPickDirectory,
  onBack,
  onFinish,
  onInstallSkill,
  onBrowseFeaturedSkills,
}: DoneStepProps) {
  const intl = useIntl()
  const currentTask = TASKS.find(t => t.id === task)!
  return (
    <WelcomeCard
      title={intl.formatMessage({ id: 'welcome.done.title' })}
      subtitle={intl.formatMessage({ id: 'welcome.done.subtitle' })}
      footer={
        <>
          <Button variant="ghost" onClick={onBack} className="px-lg py-sm text-on-surface-variant hover:text-primary font-label-md cursor-pointer focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary rounded">
            {intl.formatMessage({ id: 'welcome.done.back' })}
          </Button>
          <Button
            onClick={onFinish}
            className="px-lg py-sm bg-primary text-on-primary rounded-lg font-label-md cursor-pointer hover:bg-primary/90 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary"
          >
            {intl.formatMessage({ id: 'welcome.done.start' })}
          </Button>
        </>
      }
    >
      {/* Summary */}
      <div className="bg-surface-container-low rounded-xl p-md mb-md">
        <div className="font-label-sm text-on-surface-variant mb-xs">{intl.formatMessage({ id: 'welcome.done.setup.label' })}</div>
        <ul className="space-y-xs text-body-sm text-on-surface">
          <li className="flex items-center gap-sm">
            <span className="material-symbols-outlined text-[18px] text-primary">{currentTask.icon}</span>
            <span>{intl.formatMessage({ id: currentTask.labelKey })}</span>
          </li>
          <li className="flex items-center gap-sm">
            <span className="material-symbols-outlined text-[18px] text-primary">memory</span>
            <span>{PROVIDERS.find(p => p.id === provider)?.label ?? provider}</span>
          </li>
          <li className="flex items-center gap-sm">
            <span className="material-symbols-outlined text-[18px] text-primary">build</span>
            <span>{intl.formatMessage({ id: 'welcome.done.setup.tools' }, { count: enabledToolCount })}</span>
          </li>
        </ul>
      </div>

      {/* Optional workspace picker */}
      <div className="bg-surface-container-low rounded-xl p-md mb-md">
        <div className="font-label-sm text-on-surface-variant mb-xs">{intl.formatMessage({ id: 'welcome.done.workingDir.label' })}</div>
        <div className="font-mono text-on-surface text-sm break-all mb-sm">
          {pickedDir ?? fallbackWorkingDir ?? intl.formatMessage({ id: 'welcome.done.workingDir.default' })}
        </div>
        <Button
          variant="outline"
          onClick={onPickDirectory}
          className="px-md py-sm bg-surface-container-low hover:bg-surface-container-high border border-outline-variant/50 rounded-lg font-label-md text-on-surface cursor-pointer transition-colors flex items-center gap-sm focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
        >
          <span className="material-symbols-outlined text-[18px]">folder_open</span>
          {pickedDir
            ? intl.formatMessage({ id: 'welcome.done.workingDir.chooseOther' })
            : intl.formatMessage({ id: 'welcome.done.workingDir.choose' })}
        </Button>
      </div>

      {/* Shortcuts */}
      <div className="space-y-sm">
        <div className="font-label-md text-on-surface-variant mb-xs">{intl.formatMessage({ id: 'welcome.done.shortcuts.label' })}</div>
        {SHORTCUT_ROWS.map(s => {
          const keys = s.keys()
          return (
            <div key={s.actionKey} className="flex items-center justify-between py-xs">
              <span className="font-body-sm text-on-surface-variant">{intl.formatMessage({ id: s.actionKey })}</span>
              <kbd className="text-[11px] px-1.5 py-0.5 rounded bg-surface-container-high text-on-surface-variant font-mono shrink-0">{keys}</kbd>
            </div>
          )
        })}
      </div>
      <p className="font-body-sm text-on-surface-variant mt-md">
        {intl.formatMessage(
          { id: 'welcome.done.shortcuts.help' },
          {
            key: (chunks: React.ReactNode) => (
              <kbd className="text-[11px] px-1.5 py-0.5 rounded bg-surface-container-high text-on-surface-variant font-mono">{chunks}</kbd>
            ),
          },
        )}
      </p>

      {/* Developer mode opt-in */}
      <label className="mt-md flex items-start gap-sm p-md rounded-xl border border-outline-variant/50 hover:border-primary/50 cursor-pointer transition-all">
        <input
          type="checkbox"
          checked={devMode}
          onChange={() => setDevMode(v => !v)}
          className="mt-xs accent-primary"
          aria-label={intl.formatMessage({ id: 'welcome.done.devMode.aria' })}
        />
        <div>
          <div className="font-headline-md text-on-surface">{intl.formatMessage({ id: 'welcome.done.devMode.title' })}</div>
          <div className="font-body-sm text-on-surface-variant mt-xs">
            {intl.formatMessage({ id: 'welcome.done.devMode.desc' })}
          </div>
        </div>
      </label>

      {/* P2.4 — Documents skill recommendations. Hidden until the skill
          repos are published. */}
      {DOCUMENTS_SKILLS_AVAILABLE && (
        <DocumentsSkillsList
          skillState={skillState}
          onInstall={onInstallSkill}
          onBrowseLater={onBrowseFeaturedSkills}
        />
      )}
    </WelcomeCard>
  )
}