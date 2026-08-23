// InstallDialog body for `git_hub_repo`. Shows repo + ref, flags floating
// branches, and a busy install button that calls the matching
// `installSkillFromRepo` / `installAgentFromRepo` API.
//
// Extracted from InstallDialog.tsx (T3.1).

import { FormattedMessage, useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'

interface GitHubBodyProps {
  repo: string
  ref_: string
  installing: boolean
  onInstall: () => void
}

export function GitHubBody({ repo, ref_, installing, onInstall }: GitHubBodyProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  // Floating-branch heuristic: SHA refs are 7-40 hex chars; anything else is
  // a branch name that the registry can rotate out from under us.
  const isFloatingBranch = !/^[0-9a-f]{7,40}$/i.test(ref_)

  return (
    <div className="flex flex-col gap-md">
      <div className="flex items-center gap-sm">
        <span className="material-symbols-outlined text-[18px] text-on-surface-variant">
          code
        </span>
        <span className="font-body-md font-mono text-on-surface">{repo}</span>
      </div>
      <div className="flex items-center gap-sm">
        <span className="material-symbols-outlined text-[18px] text-on-surface-variant">
          commit
        </span>
        <span className="font-body-md font-mono text-on-surface">{ref_}</span>
        {isFloatingBranch ? (
          <span className="inline-flex items-center gap-[4px] px-xs py-[2px] rounded bg-tertiary-container/50 text-on-tertiary-container text-label-xs font-bold">
            <span className="material-symbols-outlined icon-xs">warning</span>
            {t('extensions.installDialog.floatingBranch')}
          </span>
        ) : null}
      </div>
      {isFloatingBranch ? (
        <p className="text-label-xs text-on-surface-variant">
          {t('extensions.installDialog.floatingBranchHelp')}
        </p>
      ) : null}
      <Button
        type="button"
        onClick={onInstall}
        disabled={installing}
        className="px-md py-sm rounded-lg hover:bg-primary/90 disabled:opacity-60 cursor-pointer"
      >
        <span className="material-symbols-outlined icon-sm">
          {installing ? 'progress_activity' : 'download'}
        </span>
        {installing ? (
          <FormattedMessage id="extensions.installDialog.installing" />
        ) : (
          <FormattedMessage id="extensions.installDialog.install" />
        )}
      </Button>
    </div>
  )
}