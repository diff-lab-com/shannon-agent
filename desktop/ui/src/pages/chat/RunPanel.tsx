// RunPanel — GB P2-3: the right dock's 运行 tab, the chat counterpart of
// ChatGPT Work plan/sources/files/summary. While (and after) a streaming
// task runs on the visible session it aggregates the 「过程四要素」:
//   * 一句话进度摘要 — lib/runProcess's summary (latest progress line / tool)
//   * 当前计划     — the engine's plan doc via the Plan tab's own hook
//   * 引用来源     — @refs + attachments + tool-read files
//   * 新产出文件   — write-like tools' paths
// All data comes from the existing event stream (AppContext → ChatContext)
// and the plan fetch the 计划 tab already performs — no new backend state.
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { useT } from '@/i18n'
import { useSessionPlan } from '@/pages/chat/PlanPanel'
import type { RunProcessState } from '@/lib/runProcess'
import { cn } from '@/lib/utils'
import type { ReactNode } from 'react'

interface RunPanelProps {
  run: RunProcessState
  /** Session working dir — feeds the plan fetch. */
  workingDir: string | null
  /** Switch the dock to the full 计划 tab. */
  onOpenPlan: () => void
}

function StatusBadge({ status }: { status: RunProcessState['status'] }) {
  const t = useT()
  if (status === 'idle') return null
  const map = {
    running: { key: 'chat.run.status.running', icon: 'progress_activity', className: 'bg-primary-container text-on-primary-container' },
    done: { key: 'chat.run.status.done', icon: 'check_circle', className: 'bg-tertiary-container text-on-tertiary-container' },
    failed: { key: 'chat.run.status.failed', icon: 'error', className: 'bg-error-container text-on-error-container' },
    idle: null,
  } as const
  const meta = map[status]
  if (!meta) return null
  return (
    <Badge size="sm" variant="neutral" className={cn('shrink-0', meta.className)}>
      <span className={cn('material-symbols-outlined icon-xs mr-[2px]', status === 'running' && 'animate-spin')} aria-hidden="true">
        {meta.icon}
      </span>
      {t(meta.key)}
    </Badge>
  )
}

function PathList({ paths, empty }: { paths: string[]; empty: string }) {
  if (paths.length === 0) {
    return <p className="font-label-sm text-on-surface-variant/70 px-sm">{empty}</p>
  }
  return (
    <ul className="space-y-[2px]" data-testid="run-path-list">
      {paths.map(path => (
        <li key={path} className="flex items-center gap-xs px-sm py-[2px] rounded-md hover:bg-surface-container min-w-0">
          <span className="material-symbols-outlined icon-sm text-on-surface-variant shrink-0" aria-hidden="true">draft</span>
          <span className="font-mono font-label-sm text-on-surface truncate min-w-0" title={path}>
            {path}
          </span>
        </li>
      ))}
    </ul>
  )
}

function Section({ icon, title, children }: { icon: string; title: string; children: ReactNode }) {
  return (
    <section className="rounded-xl border border-outline-variant/10 bg-surface-container overflow-hidden">
      <h4 className="flex items-center gap-xs px-sm py-xs font-label-sm uppercase tracking-wider text-on-surface-variant border-b border-outline-variant/10">
        <span className="material-symbols-outlined icon-sm" aria-hidden="true">{icon}</span>
        <span className="truncate">{title}</span>
      </h4>
      <div className="py-xs">{children}</div>
    </section>
  )
}

export default function RunPanel({ run, workingDir, onOpenPlan }: RunPanelProps) {
  const t = useT()
  // The plan element reuses the 计划 tab's fetch — one session plan doc,
  // same refresh triggers, zero new state.
  const { plan } = useSessionPlan(workingDir)

  return (
    <div className="space-y-md" data-testid="run-panel">
      {/* ① 一句话进度摘要 */}
      <div
        role="status"
        data-testid="run-summary"
        className="flex items-center gap-xs px-md py-xs rounded-lg bg-surface-container border border-outline-variant/10"
      >
        <span className="material-symbols-outlined icon-sm text-primary shrink-0" aria-hidden="true">monitoring</span>
        <span className="font-label-sm text-on-surface truncate flex-1 min-w-0" title={run.summary ?? undefined}>
          {run.summary ?? t('chat.run.idle')}
        </span>
        <StatusBadge status={run.status} />
        {run.toolCount > 0 && (
          <span className="font-mono font-label-xs tabular-nums text-on-surface-variant shrink-0">
            {t('chat.run.tools', { count: run.toolCount })}
          </span>
        )}
      </div>

      {/* ② 当前计划 — Plan tab data, one click away */}
      <Section icon="route" title={t('chat.run.section.plan')}>
        {plan?.content?.trim() ? (
          <div className="px-sm py-xs space-y-xs">
            <div className="font-label-sm text-on-surface truncate" title={plan.title}>{plan.title}</div>
            <Button
              variant="outline"
              size="sm"
              onClick={onOpenPlan}
              className="rounded-sm gap-xs"
              aria-label={t('chat.run.plan.view')}
            >
              <span className="material-symbols-outlined icon-sm" aria-hidden="true">route</span>
              {t('chat.run.plan.view')}
            </Button>
          </div>
        ) : (
          <p className="font-label-sm text-on-surface-variant/70 px-sm">{t('chat.run.plan.none')}</p>
        )}
      </Section>

      {/* ③ 引用来源 — @refs, attachments, tool-read files */}
      <Section icon="source" title={t('chat.run.section.sources')}>
        <PathList paths={run.sources} empty={t('chat.run.empty.sources')} />
      </Section>

      {/* ④ 新产出文件 — write-like tools' paths */}
      <Section icon="new releases" title={t('chat.run.section.outputs')}>
        <PathList paths={run.outputs} empty={t('chat.run.empty.outputs')} />
      </Section>
    </div>
  )
}
