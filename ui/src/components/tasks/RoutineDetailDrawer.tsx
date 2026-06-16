// RoutineDetailDrawer — Phase D C4 deliverable.
//
// Right-side drawer for inspecting and editing a scheduled routine.
// Currently exposes the DependsOnEditor; later phases can add prompt /
// trigger / policy editing here too. Click backdrop or close button to
// dismiss. Escape closes too.

import { useIntl } from 'react-intl'
import type { ScheduledRoutine } from '@/types'
import DependsOnEditor from './DependsOnEditor'

interface RoutineDetailDrawerProps {
  routine: ScheduledRoutine | null
  routines: ScheduledRoutine[]
  onClose: () => void
  onUpdated?: (routine: ScheduledRoutine) => void
}

function formatTimestamp(ts?: number | null): string {
  if (!ts) return '—'
  return new Date(ts * 1000).toLocaleString()
}

export default function RoutineDetailDrawer({
  routine,
  routines,
  onClose,
  onUpdated,
}: RoutineDetailDrawerProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  if (!routine) return null
  const deps = (routine.depends_on ?? []).map(id => routines.find(r => r.id === id)?.name ?? id)
  return (
    <div
      className="fixed inset-0 z-50 flex justify-end"
      onClick={onClose}
      onKeyDown={e => { if (e.key === 'Escape') onClose() }}
      role="dialog"
      aria-modal="true"
      aria-label={intl.formatMessage({ id: 'tasks.routineDetailDrawer.ariaLabel' }, { name: routine.name })}
    >
      <div className="bg-black/20 absolute inset-0" />
      <div
        className="relative w-[440px] bg-surface-container-lowest shadow-2xl border-l border-outline-variant/20 p-xl overflow-y-auto"
        onClick={e => e.stopPropagation()}
      >
        <div className="flex items-center justify-between mb-lg">
          <h3 className="font-headline-md text-on-surface font-bold">{t('tasks.routineDetailDrawer.title')}</h3>
          <button
            aria-label={t('tasks.routineDetailDrawer.closeAria')}
            className="p-sm rounded-lg hover:bg-surface-container text-on-surface-variant cursor-pointer focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
            onClick={onClose}
          >
            <span className="material-symbols-outlined">close</span>
          </button>
        </div>
        <div className="space-y-md">
          <div>
            <span className="text-label-sm text-on-surface-variant">{t('tasks.routineDetailDrawer.name')}</span>
            <p className="font-body-lg text-on-surface font-bold mt-xs">{routine.name}</p>
          </div>
          <div>
            <span className="text-label-sm text-on-surface-variant">{t('tasks.routineDetailDrawer.prompt')}</span>
            <p className="font-body-md text-on-surface mt-xs whitespace-pre-wrap break-words">
              {routine.prompt}
            </p>
          </div>
          <div className="grid grid-cols-2 gap-md">
            <div>
              <span className="text-label-sm text-on-surface-variant">{t('tasks.routineDetailDrawer.trigger')}</span>
              <p className="font-body-md text-on-surface mt-xs capitalize">
                {routine.trigger_type.charAt(0).toUpperCase() + routine.trigger_type.slice(1)}
              </p>
            </div>
            <div>
              <span className="text-label-sm text-on-surface-variant">{t('tasks.routineDetailDrawer.enabled')}</span>
              <p className="font-body-md text-on-surface mt-xs">{routine.enabled ? t('tasks.routineDetailDrawer.yes') : t('tasks.routineDetailDrawer.no')}</p>
            </div>
            <div>
              <span className="text-label-sm text-on-surface-variant">{t('tasks.routineDetailDrawer.nextFire')}</span>
              <p className="font-body-md text-on-surface mt-xs">{formatTimestamp(routine.next_fire_at)}</p>
            </div>
            <div>
              <span className="text-label-sm text-on-surface-variant">{t('tasks.routineDetailDrawer.lastFire')}</span>
              <p className="font-body-md text-on-surface mt-xs">{formatTimestamp(routine.last_fired)}</p>
            </div>
          </div>
          {routine.last_error && (
            <div className="rounded-xl border border-error/20 bg-error/10 px-md py-sm">
              <span className="text-label-sm text-error">{t('tasks.routineDetailDrawer.lastError')}</span>
              <p className="font-body-md text-error mt-xs break-words">{routine.last_error}</p>
            </div>
          )}
          {routine.policy?.result_routing && routine.policy.result_routing.length > 0 ? (
            <div>
              <span className="text-label-sm text-on-surface-variant">{t('tasks.routineDetailDrawer.resultRouting')}</span>
              <ul className="flex flex-wrap gap-xs mt-xs">
                {routine.policy.result_routing.map(entry => (
                  <li
                    key={entry}
                    className="font-label-sm text-[11px] bg-tertiary/10 text-tertiary px-sm py-0.5 rounded-full border border-tertiary/30"
                  >
                    {entry}
                  </li>
                ))}
              </ul>
            </div>
          ) : null}
          <div>
            <div className="flex items-center justify-between mb-sm">
              <span className="text-label-sm text-on-surface-variant uppercase tracking-wider">
                {t('tasks.routineDetailDrawer.dependencies')}
              </span>
              <span className="font-label-sm text-[11px] text-on-surface-variant">
                {deps.length === 0 ? t('tasks.routineDetailDrawer.none') : deps.join(', ')}
              </span>
            </div>
            <DependsOnEditor routine={routine} routines={routines} onUpdated={onUpdated} />
          </div>
        </div>
      </div>
    </div>
  )
}
