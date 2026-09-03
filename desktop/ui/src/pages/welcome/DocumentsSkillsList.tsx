// Optional P2.4 documents skills recommendations. Gated behind
// DOCUMENTS_SKILLS_AVAILABLE — the section is hidden until the skill repos
// are published. Extracted from Welcome.tsx (T3.1).
import { useIntl } from 'react-intl'
import { Spinner } from '@/components/ui/loading-state'
import { Button } from '@/components/ui/button'
import { DOCUMENTS_SKILLS, type DocumentsSkill } from './constants'

interface SkillState {
  status: 'idle' | 'installing' | 'installed' | 'failed'
  error?: string
}

interface DocumentsSkillsListProps {
  skillState: Record<string, SkillState>
  onInstall: (skill: DocumentsSkill) => void
  onBrowseLater: () => void
}

export function DocumentsSkillsList({ skillState, onInstall, onBrowseLater }: DocumentsSkillsListProps) {
  const intl = useIntl()
  return (
    <div className="mt-md p-md rounded-xl border border-outline-variant/50 bg-surface-container-low">
      <div className="flex items-center gap-xs mb-xs">
        <span className="material-symbols-outlined text-primary text-[20px]">extension</span>
        <span className="font-headline-md text-on-surface">
          {intl.formatMessage({ id: 'welcome.skills.title' })}
        </span>
      </div>
      <p className="font-body-sm text-on-surface-variant mb-md">
        {intl.formatMessage({ id: 'welcome.skills.subtitle' })}
      </p>
      <ul className="space-y-sm">
        {DOCUMENTS_SKILLS.map(skill => {
          const state = skillState[skill.id] ?? { status: 'idle' as const }
          return (
            <li
              key={skill.id}
              className="flex items-start gap-sm p-sm rounded-lg bg-surface-container-lowest border border-outline-variant/30"
            >
              <span className="material-symbols-outlined text-on-surface-variant text-[20px] mt-[2px] shrink-0">
                {skill.icon}
              </span>
              <div className="flex-1 min-w-0">
                <div className="font-label-md text-on-surface">{intl.formatMessage({ id: skill.labelKey })}</div>
                <div className="font-body-sm text-on-surface-variant mt-[2px]">
                  {intl.formatMessage({ id: skill.descKey })}
                </div>
                {state.status === 'failed' && state.error && (
                  <div className="font-body-sm text-error mt-xs">{state.error}</div>
                )}
              </div>
              <Button
                type="button"
                variant="outline"
                onClick={() => onInstall(skill)}
                disabled={state.status === 'installing' || state.status === 'installed'}
                className="shrink-0 px-md py-xs rounded-lg font-label-md text-label-sm bg-surface-container-high hover:bg-surface-container-highest border border-outline-variant/50 text-on-surface cursor-pointer transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-xs"
                aria-label={intl.formatMessage({ id: 'welcome.skills.install.aria' }, { name: intl.formatMessage({ id: skill.labelKey }) })}
              >
                {state.status === 'installing' && (
                  <Spinner className="text-[14px]" />
                )}
                {state.status === 'installed' ? (
                  <span className="material-symbols-outlined text-[14px]">check</span>
                ) : state.status === 'installing' ? (
                  intl.formatMessage({ id: 'welcome.skills.installing' })
                ) : (
                  intl.formatMessage({ id: 'welcome.skills.install' })
                )}
              </Button>
            </li>
          )
        })}
      </ul>
      <p className="font-body-sm text-on-surface-variant mt-md">
        {intl.formatMessage(
          { id: 'welcome.skills.later' },
          {
            link: (chunks: React.ReactNode) => (
              <Button
                variant="link"
                onClick={onBrowseLater}
                className="text-primary hover:underline cursor-pointer p-0 h-auto"
              >
                {chunks}
              </Button>
            ),
          },
        )}
      </p>
    </div>
  )
}