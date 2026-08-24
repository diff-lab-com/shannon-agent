// RoutineDetailDrawer — Phase D C4 deliverable.
//
// Right-side drawer for inspecting and editing a scheduled routine.
// Currently exposes the DependsOnEditor; later phases can add prompt /
// trigger / policy editing here too.
//
// T1.2 — migrated onto the shared <SidePanel> primitive. The previous
// hand-rolled overlay, document-level Escape listener, backdrop click
// handler, and focus trap hook are all gone: SidePanel owns them.
// The `aria-label` interpolates the routine name; that wiring lives
// on SidePanel's `ariaLabel` prop.

import { useIntl } from 'react-intl'
import type { ScheduledRoutine } from '@/types'
import DependsOnEditor from './DependsOnEditor'
import { SidePanel, SidePanelBody, SidePanelCloseButton, SidePanelHeader, SidePanelTitle } from '@/components/ui/side-panel'

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
  const t = (id: string, values?: Record<string, string | number>) =>
    intl.formatMessage({ id }, values)

  if (!routine) return null
  const deps = (routine.depends_on ?? []).map(id => routines.find(r => r.id === id)?.name ?? id)
  const ariaLabel = t('tasks.routineDetailDrawer.ariaLabel', { name: routine.name })
  const closeAria = t('tasks.routineDetailDrawer.closeAria')

  return (
    <SidePanel
      open={!!routine}
      onClose={onClose}
      ariaLabel={ariaLabel}
      width="440px"
    >
      <SidePanelHeader>
        <SidePanelTitle>{t('tasks.routineDetailDrawer.title')}</SidePanelTitle>
        <SidePanelCloseButton onClick={onClose} label={closeAria} />
      </SidePanelHeader>
      <SidePanelBody>
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
      </SidePanelBody>
    </SidePanel>
  )
}