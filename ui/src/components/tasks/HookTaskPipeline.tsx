// HookTaskPipeline — Phase D P3.3 deliverable.
//
// Surfaces triggered routines (hook-event-driven task automations) registered
// on the backend. Each row shows the hook event as a colored badge, the
// matcher/pattern, the command that fires, and an enable/disable toggle.
//
// Routines are loaded via listTriggeredRoutines() and toggled via
// toggleTriggeredRoutine(name, enabled). The list is refreshed on mount and
// on manual refresh.

import { useEffect, useState, useCallback } from 'react'
import { useIntl } from 'react-intl'
import * as api from '@/lib/tauri-api'
import type { TriggeredRoutineDto } from '@/types'
import LoadingState from '@/components/ui/loading-state'
import HookRoutineCreateDialog from './HookRoutineCreateDialog'

const HOOK_BADGE: Record<string, { icon: string; tone: string }> = {
  pretooluse: { icon: 'lock', tone: 'bg-secondary/15 text-secondary border-secondary/40' },
  posttooluse: { icon: 'check_circle', tone: 'bg-tertiary/15 text-tertiary border-tertiary/40' },
  subagentstart: { icon: 'rocket_launch', tone: 'bg-primary/15 text-primary border-primary/40' },
  subagentstop: { icon: 'stop_circle', tone: 'bg-outline/15 text-on-surface-variant border-outline/40' },
  precompact: { icon: 'compress', tone: 'bg-secondary/15 text-secondary border-secondary/40' },
  postcompact: { icon: 'expand', tone: 'bg-tertiary/15 text-tertiary border-tertiary/40' },
  configchange: { icon: 'settings', tone: 'bg-error/15 text-error border-error/40' },
  taskcreated: { icon: 'add_task', tone: 'bg-primary/15 text-primary border-primary/40' },
  taskcompleted: { icon: 'task_alt', tone: 'bg-tertiary/15 text-tertiary border-tertiary/40' },
}

function badgeFor(trigger: string): { icon: string; tone: string; label: string } {
  const key = trigger.toLowerCase().replace(/[^a-z]/g, '')
  const found = HOOK_BADGE[key]
  if (found) return { ...found, label: trigger }
  return { icon: 'bolt', tone: 'bg-outline/15 text-on-surface-variant border-outline/40', label: trigger }
}

export default function HookTaskPipeline() {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  const [routines, setRoutines] = useState<TriggeredRoutineDto[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [toggling, setToggling] = useState<Set<string>>(new Set())
  const [createOpen, setCreateOpen] = useState(false)

  const refresh = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const list = await api.listTriggeredRoutines()
      setRoutines(list)
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => { refresh() }, [refresh])

  const onToggle = async (name: string, enabled: boolean) => {
    setToggling(prev => new Set(prev).add(name))
    try {
      await api.toggleTriggeredRoutine(name, enabled)
      setRoutines(prev => prev.map(r => r.name === name ? { ...r, enabled } : r))
    } catch (e) {
      setError(intl.formatMessage({ id: 'tasks.hookTaskPipeline.toggleFailed' }, { name, error: String(e) }))
    } finally {
      setToggling(prev => {
        const next = new Set(prev)
        next.delete(name)
        return next
      })
    }
  }

  const enabledCount = routines.filter(r => r.enabled).length

  return (
    <div className="bg-surface-container-lowest rounded-2xl p-lg border border-outline-variant/30 shadow-sm flex flex-col gap-md">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-sm">
          <span className="material-symbols-outlined text-[20px] text-on-surface">conversion_path</span>
          <h3 className="font-headline-md text-[16px] font-bold text-on-surface">{t('tasks.hookTaskPipeline.title')}</h3>
          {routines.length > 0 ? (
            <span className="font-label-sm text-[11px] text-on-surface-variant bg-surface-container-low px-xs py-1 rounded-full">
              {intl.formatMessage({ id: 'tasks.hookTaskPipeline.active' }, { enabled: enabledCount, total: routines.length })}
            </span>
          ) : null}
        </div>
        <div className="flex items-center gap-xs">
          <button
            type="button"
            onClick={() => setCreateOpen(true)}
            className="font-label-sm text-primary hover:bg-primary/10 rounded px-sm py-xs cursor-pointer flex items-center gap-1 focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
            aria-label={t('routines.create.aria')}
          >
            <span className="material-symbols-outlined text-[14px]">add</span>
            {t('tasks.hookTaskPipeline.add')}
          </button>
          <button
            type="button"
            onClick={refresh}
            disabled={loading}
            className="font-label-sm text-primary hover:bg-primary/10 rounded px-sm py-xs cursor-pointer flex items-center gap-1 focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 disabled:opacity-40"
          >
            <span className="material-symbols-outlined text-[14px]">{loading ? 'hourglass_top' : 'refresh'}</span>
            {t('tasks.hookTaskPipeline.refresh')}
          </button>
        </div>
      </div>

      {error ? (
        <div className="font-label-sm text-error flex items-center gap-sm">
          <span className="material-symbols-outlined text-[14px]">error</span>
          {error}
        </div>
      ) : null}

      {loading && routines.length === 0 ? (
        <LoadingState size="sm" label={t('tasks.hookTaskPipeline.loading')} />
      ) : routines.length === 0 ? (
        <div className="text-center py-lg">
          <span className="material-symbols-outlined text-[32px] text-on-surface-variant/40 block mb-sm">link_off</span>
          <p className="text-body-sm text-on-surface-variant">
            {t('tasks.hookTaskPipeline.empty')}
          </p>
          <p className="font-label-sm text-[11px] text-on-surface-variant mt-xs">
            {t('tasks.hookTaskPipeline.emptyDesc')}
          </p>
        </div>
      ) : (
        <ul className="flex flex-col gap-sm" aria-label={t('tasks.hookTaskPipeline.title')}>
          {routines.map(r => {
            const b = badgeFor(r.trigger)
            const isToggling = toggling.has(r.name)
            return (
              <li
                key={r.name}
                className={`flex items-start gap-sm p-sm rounded-lg border ${
                  r.enabled
                    ? 'border-outline-variant/30 bg-surface-container-low'
                    : 'border-outline-variant/20 bg-surface-container-low/40 opacity-70'
                }`}
              >
                <span className={`inline-flex items-center gap-1 px-xs py-1 rounded-full border font-label-sm text-[10px] font-bold uppercase tracking-wide ${b.tone}`}>
                  <span className="material-symbols-outlined text-[12px]">{b.icon}</span>
                  {b.label}
                </span>
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-xs">
                    <span className="font-label-md text-on-surface truncate">{r.name}</span>
                    {r.matcher ? (
                      <code className="font-mono font-label-sm text-[10px] text-on-surface-variant bg-surface-container-high/60 px-1 rounded">{r.matcher}</code>
                    ) : null}
                    {r.pattern ? (
                      <code className="font-mono font-label-sm text-[10px] text-tertiary bg-tertiary/10 px-1 rounded">~/{r.pattern}/</code>
                    ) : null}
                  </div>
                  {r.description ? (
                    <p className="font-label-sm text-[11px] text-on-surface-variant mt-1">{r.description}</p>
                  ) : null}
                  <code className="font-mono font-label-sm text-[11px] text-on-surface-variant block mt-1 truncate">
                    $ {r.command}
                  </code>
                </div>
                <button
                  type="button"
                  role="switch"
                  aria-checked={r.enabled}
                  aria-label={intl.formatMessage({ id: 'tasks.hookTaskPipeline.toggleAria' }, { name: r.name })}
                  disabled={isToggling}
                  onClick={() => onToggle(r.name, !r.enabled)}
                  className={`relative inline-flex h-5 w-9 shrink-0 cursor-pointer rounded-full transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 disabled:opacity-40 ${
                    r.enabled ? 'bg-primary' : 'bg-outline-variant'
                  }`}
                >
                  <span className={`inline-block h-4 w-4 bg-white rounded-full shadow transition-transform absolute top-0.5 ${r.enabled ? 'translate-x-4' : 'translate-x-0.5'}`} />
                </button>
              </li>
            )
          })}
        </ul>
      )}

      <HookRoutineCreateDialog
        open={createOpen}
        onClose={() => setCreateOpen(false)}
        onCreated={() => refresh()}
      />
    </div>
  )
}
