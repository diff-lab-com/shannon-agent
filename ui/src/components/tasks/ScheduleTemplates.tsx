// ScheduleTemplates — Phase D P3.2 deliverable.
//
// Preset templates that pre-fill ScheduleForm fields. Clicking a chip calls
// onApply with a partial payload. Each template encodes a sensible default
// for the named scenario; the user can still edit any field afterwards.

import type { TriggerType } from '@/types'

export interface ScheduleTemplate {
  id: string
  name: string
  icon: string
  description: string
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
    name: 'Daily Standup Summary',
    icon: 'groups',
    description: 'Aggregate yesterday\'s commits + open PRs into a standup digest.',
    fields: {
      name: 'Daily Standup',
      prompt: 'Summarize yesterday\'s commits across all branches, list open PRs needing review, and flag any blockers from in-progress tasks.',
      trigger_type: 'cron',
      cron_expr: '0 9 * * *',
    },
  },
  {
    id: 'weekly-deps',
    name: 'Weekly Dependency Scan',
    icon: 'security',
    description: 'Run cargo audit + npm audit and route findings to triage.',
    fields: {
      name: 'Weekly Dependency Scan',
      prompt: 'Run cargo audit and npm audit. Triage any vulnerabilities by severity and open issues for critical findings.',
      trigger_type: 'cron',
      cron_expr: '0 6 * * 1',
    },
  },
  {
    id: 'pr-auto-review',
    name: 'PR Auto-Review',
    icon: 'rate_review',
    description: 'Sweep open PRs and post a first-pass review comment.',
    fields: {
      name: 'PR Auto-Review',
      prompt: 'For every open PR updated in the last 24h, post a review comment covering style, tests, and risk.',
      trigger_type: 'interval',
      interval_secs: 6 * 3600,
    },
  },
  {
    id: 'changelog',
    name: 'Changelog Generator',
    icon: 'change_history',
    description: 'Generate a weekly changelog from merged PRs.',
    fields: {
      name: 'Weekly Changelog',
      prompt: 'Collect all PRs merged since last Monday, group by category (feature/fix/chore), and draft a Markdown changelog.',
      trigger_type: 'cron',
      cron_expr: '0 17 * * 5',
    },
  },
  {
    id: 'nightly-tests',
    name: 'Nightly Test Suite',
    icon: 'science',
    description: 'Run the full test suite in a clean worktree.',
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
  return (
    <div className="flex flex-col gap-sm mb-md">
      <div className="font-label-md text-on-surface-variant flex items-center gap-xs">
        <span className="material-symbols-outlined text-[14px]">auto_awesome</span>
        Templates
      </div>
      <div className="flex flex-wrap gap-xs">
        {SCHEDULE_TEMPLATES.map(t => (
          <button
            key={t.id}
            type="button"
            title={t.description}
            onClick={() => onApply(t)}
            className="flex items-center gap-xs px-sm py-xs rounded-full border border-outline-variant/30 bg-surface-container-low hover:bg-primary/10 hover:border-primary/40 text-on-surface-variant hover:text-primary font-label-sm text-[12px] transition-colors cursor-pointer focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
          >
            <span className="material-symbols-outlined text-[14px]">{t.icon}</span>
            {t.name}
          </button>
        ))}
      </div>
    </div>
  )
}
