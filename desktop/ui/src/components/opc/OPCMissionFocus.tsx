import { useMemo, useState, useEffect } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { toastError } from '@/lib/errorToast'
import * as api from '@/lib/tauri-api'
import { classifyStatus } from '@/lib/task-status'
import type { TaskItem } from '@/types'

interface Props {
  config: { provider?: string; strategic_focus?: string } | null
  /** Board tasks — the hero aggregates 进行中/已完成 counts + progress from
   *  them (design 09:169-181). 预算/截止 wait on the 「使命」 entity (audit
   *  B3: 单独立项), so the chips row stays honest about what exists. */
  tasks: TaskItem[]
}

export default function OPCMissionFocus({ config, tasks }: Props) {
  const intl = useIntl()
  const [editing, setEditing] = useState(false)
  const [text, setText] = useState('')

  const focus = config?.strategic_focus
    || (config?.provider
      ? `${config.provider.charAt(0).toUpperCase() + config.provider.slice(1)} Agent Orchestration — autonomous task execution with multi-agent coordination.`
      : intl.formatMessage({ id: 'opc.missionFocus.defaultFocus' }))

  useEffect(() => { setText(focus) }, [focus])

  // 使命 hero 三要素的纯前端部分 (audit §09 P1): counts + progress bar +
  // aurora line — all derived from the tasks the page already holds.
  // Catalog may not have hydrated (tests, degraded states) — never assume
  // the array exists (OPCMissionFocus tests mock useCatalog without tasks).
  const safeTasks = useMemo(() => tasks ?? [], [tasks])
  const { activeCount, doneCount, progressPct } = useMemo(() => {
    let active = 0
    let done = 0
    for (const task of safeTasks) {
      const family = classifyStatus(task.status)
      if (family === 'active') active++
      else if (family === 'done') done++
    }
    return {
      activeCount: active,
      doneCount: done,
      progressPct: safeTasks.length > 0 ? Math.round((done / safeTasks.length) * 100) : 0,
    }
  }, [safeTasks])

  const save = () => {
    api.configure({ key: 'strategic_focus', value: text })
      .then(() => toast.success(intl.formatMessage({ id: 'opc.missionFocus.focusSaved' })))
      .catch((e) => toastError(intl.formatMessage({ id: 'opc.missionFocus.saveFailed' }), e))
    setEditing(false)
  }

  return (
    <div className="bg-surface-container-lowest rounded-2xl p-xl mb-lg border border-outline-variant/30 relative shadow-e1 aurora-line">
      <div className="flex items-start gap-md">
        <span className="material-symbols-outlined text-primary mt-xs" style={{ fontSize: 30 }} aria-hidden="true">
          target
        </span>
        <div className="flex-1 min-w-0">
          <div className="flex items-center justify-between mb-sm">
            <div className="flex items-center gap-sm uppercase font-label-md text-label-sm tracking-widest text-on-surface-variant font-bold">
              <span className="w-1.5 h-1.5 bg-outline-variant rotate-45 block" />
              {intl.formatMessage({ id: 'opc.missionFocus.todayMission' })}
            </div>
            <Button
              variant="link"
              size="sm"
              className="text-label-sm h-auto px-0 hover:underline"
              onClick={() => setEditing(!editing)}
              aria-expanded={editing}
            >
              {editing ? intl.formatMessage({ id: 'opc.missionFocus.cancel' }) : intl.formatMessage({ id: 'opc.missionFocus.edit' })}
            </Button>
          </div>
          {editing ? (
            <div className="mt-sm space-y-md">
              <textarea
                className="w-full h-24 p-md bg-surface-container-low rounded-xl border border-outline-variant/30 text-body-md resize-none focus:outline-none focus:ring-2 focus:ring-primary/30"
                value={text}
                onChange={e => setText(e.target.value)}
                aria-label={intl.formatMessage({ id: 'opc.missionFocus.editMission.aria' })}
              />
              <Button
                className="px-md py-sm rounded-lg font-label-md hover:opacity-90"
                onClick={save}
              >
                {intl.formatMessage({ id: 'opc.missionFocus.saveFocus' })}
              </Button>
            </div>
          ) : (
            <h2 className="font-headline-lg text-[28px] font-bold text-on-surface mt-sm max-w-5xl">
              {focus}
            </h2>
          )}
          {/* Progress + counts strip (design 09:174-177) — visible in both
              states so the mission reads at a glance while editing too. */}
          <div
            className="mt-sm"
            role="status"
            aria-label={intl.formatMessage({ id: 'opc.missionFocus.progress.aria' }, { percent: progressPct })}
            data-testid="opc-mission-progress"
          >
            <div className="h-1.5 w-full max-w-xl bg-surface-container rounded-full overflow-hidden">
              <div
                className="h-full bg-primary rounded-full transition-all duration-(--duration-slower)"
                style={{ width: `${progressPct}%` }}
              />
            </div>
            <div className="flex items-center gap-sm mt-xs font-label-sm text-label-sm text-on-surface-variant flex-wrap">
              <span>
                {intl.formatMessage({ id: 'opc.missionFocus.activeCount' }, { count: activeCount })}
              </span>
              <span aria-hidden="true">·</span>
              <span>
                {intl.formatMessage({ id: 'opc.missionFocus.doneCount' }, { count: doneCount })}
              </span>
              <span aria-hidden="true">·</span>
              <span className="tabular-nums">
                {intl.formatMessage({ id: 'opc.missionFocus.totalCount' }, { count: safeTasks.length })}
              </span>
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}
