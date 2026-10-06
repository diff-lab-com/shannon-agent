// ScheduleTemplates — Phase D P3.2 deliverable.
//
// Preset templates that pre-fill ScheduleForm fields. Clicking a chip calls
// onApply with a partial payload. Each template encodes a sensible default
// for the named scenario; the user can still edit any field afterwards.

import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import type { TriggerType } from '@/types'

export interface ScheduleTemplate {
  id: string
  /** i18n key stem under `tasks.scheduleTemplates.*` — B6-36: the chip label
   *  and tooltip used to be hardcoded English. */
  idKey: string
  icon: string
  /** Prefill payload for ScheduleForm. Deliberately not localized: the
   *  routine name/prompt are agent instructions, shipped in English on
   *  purpose (same policy as the built-in routine templates). */
  fields: {
    name?: string
    prompt?: string
    trigger_type?: TriggerType
    interval_secs?: number
    cron_expr?: string
  }
}

export const SCHEDULE_TEMPLATES: ScheduleTemplate[] = [
  {
    id: 'daily-standup',
    idKey: 'dailyStandup',
    icon: 'groups',
    fields: {
      name: 'Daily Standup',
      prompt: 'Summarize yesterday\'s commits across all branches, list open PRs needing review, and flag any blockers from in-progress tasks.',
      trigger_type: 'cron',
      cron_expr: '0 9 * * *',
    },
  },
  {
    id: 'weekly-deps',
    idKey: 'weeklyDeps',
    icon: 'security',
    fields: {
      name: 'Weekly Dependency Scan',
      prompt: 'Run cargo audit and npm audit. Triage any vulnerabilities by severity and open issues for critical findings.',
      trigger_type: 'cron',
      cron_expr: '0 6 * * 1',
    },
  },
  {
    id: 'pr-auto-review',
    idKey: 'prAutoReview',
    icon: 'rate_review',
    fields: {
      name: 'PR Auto-Review',
      prompt: 'For every open PR updated in the last 24h, post a review comment covering style, tests, and risk.',
      trigger_type: 'interval',
      interval_secs: 6 * 3600,
    },
  },
  {
    id: 'changelog',
    idKey: 'changelog',
    icon: 'change_history',
    fields: {
      name: 'Weekly Changelog',
      prompt: 'Collect all PRs merged since last Monday, group by category (feature/fix/chore), and draft a Markdown changelog.',
      trigger_type: 'cron',
      cron_expr: '0 17 * * 5',
    },
  },
  {
    id: 'nightly-tests',
    idKey: 'nightlyTests',
    icon: 'science',
    fields: {
      name: 'Nightly Tests',
      prompt: 'Run `just ci` in an isolated worktree. Report failures with logs and open issues for any regression.',
      trigger_type: 'cron',
      cron_expr: '0 2 * * *',
    },
  },
]

interface ScheduleTemplatesProps {
  onApply: (template: ScheduleTemplate) => void
}

export default function ScheduleTemplates({ onApply }: ScheduleTemplatesProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  return (
    <div className="flex flex-col gap-sm mb-md">
      <div className="font-label-md text-on-surface-variant flex items-center gap-xs">
        <span className="material-symbols-outlined icon-sm">auto_awesome</span>
        {t('tasks.scheduleTemplates.title')}
      </div>
      <div className="flex flex-wrap gap-xs">
        {SCHEDULE_TEMPLATES.map(tpl => (
          <Button
            key={tpl.id}
            type="button"
            variant="outline"
            size="sm"
            title={t(`tasks.scheduleTemplates.${tpl.idKey}.description`)}
            onClick={() => onApply(tpl)}
            className="rounded-full border-outline-variant/30 bg-surface-container-low hover:bg-primary/10 hover:border-primary/40 text-on-surface-variant hover:text-primary font-label-sm text-label-sm"
          >
            <span className="material-symbols-outlined icon-sm">{tpl.icon}</span>
            {t(`tasks.scheduleTemplates.${tpl.idKey}.name`)}
          </Button>
        ))}
      </div>
    </div>
  )
}
