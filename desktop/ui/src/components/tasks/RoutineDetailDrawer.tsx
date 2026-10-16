// RoutineDetailDrawer — Phase D C4 deliverable.
//
// Right-side drawer for inspecting and editing a scheduled routine.
// P1-1 (G2b): the name/prompt static blocks became the RoutineBasicsEditor
// (name + prompt + trigger editing via update_scheduled_task) and the
// lifecycle row (pause/resume switch + guarded delete) joined the top of
// the body — the "can build but can't stop" gap.
//
// T1.2 — migrated onto the shared <SidePanel> primitive. The previous
// hand-rolled overlay, document-level Escape listener, backdrop click
// handler, and focus trap hook are all gone: SidePanel owns them.
// The `aria-label` interpolates the routine name; that wiring lives
// on SidePanel's `ariaLabel` prop.

import { useEffect, useState } from 'react'
import { useIntl } from 'react-intl'
import type { ScheduledRoutine } from '@/types'
import { cn } from '@/lib/utils'
import DependsOnEditor from './DependsOnEditor'
import OffpeakWindowEditor from './OffpeakWindowEditor'
import RoutineBasicsEditor from './RoutineBasicsEditor'
import RoutineLifecycleRow from './RoutineLifecycleRow'
import { SidePanel, SidePanelBody, SidePanelCloseButton, SidePanelHeader, SidePanelTitle } from '@/components/ui/side-panel'

interface RoutineDetailDrawerProps {
  routine: ScheduledRoutine | null
  routines: ScheduledRoutine[]
  onClose: () => void
  onUpdated?: (routine: ScheduledRoutine) => void
  /** 补3 (design-parity R2 §7.1): one-shot error anchor — when the drawer
      opens from Triage's 「查看日志」, scroll the last_error block into
      view and flash it (~2s ring) so the failure is the first thing the
      user sees. Only meaningful together with a routine that HAS a
      last_error; closing the drawer clears the caller's flag. */
  focusLastError?: boolean
}

/** How long the one-shot error ring stays on before fading out (补3). */
const ERROR_FLASH_MS = 2000

function formatTimestamp(ts?: number | null): string {
  if (!ts) return '—'
  return new Date(ts * 1000).toLocaleString()
}

export default function RoutineDetailDrawer({
  routine,
  routines,
  onClose,
  onUpdated,
  focusLastError = false,
}: RoutineDetailDrawerProps) {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, string | number>) =>
    intl.formatMessage({ id }, values)

  // 补3: anchor state lives in a hook BEFORE the `routine == null` early
  // return so the drawer can stay mounted (rendering null) across open/
  // close cycles without remounting its hook order. The block node arrives
  // via a callback ref (the SidePanel portal mounts its content after the
  // first commit — a plain useRef would still be null when the effect runs).
  const [errorFlash, setErrorFlash] = useState(false)
  const [errorBlock, setErrorBlock] = useState<HTMLDivElement | null>(null)
  const routineId = routine?.id ?? null
  const hasLastError = routine?.last_error != null
  useEffect(() => {
    // No hand-over flag (or nothing to anchor yet): keep the ring off.
    if (!focusLastError || !hasLastError || !errorBlock) {
      setErrorFlash(false)
      return
    }
    // Wait one frame so the drawer body has laid out before scrolling —
    // the freshly-opened panel's scroll container is not measurable yet.
    const raf = requestAnimationFrame(() => {
      errorBlock.scrollIntoView({ block: 'center' })
      setErrorFlash(true)
    })
    const timer = window.setTimeout(() => setErrorFlash(false), ERROR_FLASH_MS)
    return () => {
      cancelAnimationFrame(raf)
      window.clearTimeout(timer)
    }
  }, [focusLastError, routineId, hasLastError, errorBlock])

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
          {/* P1-1: pause/resume switch + guarded delete. The drawer-level
              onUpdated carries the routine; the sub-editors only need a
              "something changed, refresh" signal. */}
          <RoutineLifecycleRow
            key={`lifecycle-${routine.id}`}
            routine={routine}
            onUpdated={onUpdated ? () => onUpdated(routine) : undefined}
            onDeleted={onClose}
          />
          {/* P1-1: name / prompt / trigger editing. Keyed by routine id so
              the fields reset when another routine is opened. */}
          <RoutineBasicsEditor
            key={`basics-${routine.id}`}
            routine={routine}
            onUpdated={onUpdated ? () => onUpdated(routine) : undefined}
          />
          <div className="grid grid-cols-2 gap-md">
            <div>
              <span className="text-label-sm text-on-surface-variant">{t('tasks.routineDetailDrawer.enabled')}</span>
              <p className="font-body-md text-on-surface mt-xs">
                {routine.enabled ? t('tasks.routineDetailDrawer.yes') : t('tasks.routineDetailDrawer.no')}
              </p>
            </div>
            <div>
              <span className="text-label-sm text-on-surface-variant">{t('tasks.routineDetailDrawer.trigger')}</span>
              <p className="font-body-md text-on-surface mt-xs capitalize">
                {routine.trigger_type.charAt(0).toUpperCase() + routine.trigger_type.slice(1)}
              </p>
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
            <div
              ref={setErrorBlock}
              // 补3: the anchor target for Triage's 「查看日志」 hand-over —
              // the block scrolls into view and carries the one-shot ring
              // for ~2s (data-error-flash + ring class are the jsdom-safe
              // test seam).
              data-testid="routine-last-error"
              data-error-flash={errorFlash ? 'true' : undefined}
              className={cn(
                'rounded-xl border border-error/20 bg-error-container px-md py-sm transition-shadow',
                errorFlash && 'border-error ring-2 ring-error/50',
              )}
            >
              <span className="text-label-sm text-error">{t('tasks.routineDetailDrawer.lastError')}</span>
              <p className="font-body-md text-error mt-xs break-words">{routine.last_error}</p>
            </div>
          )}
          {/* P2-5: off-peak execution window editor + queued status.
              Keyed by routine id so toggles/inputs reset per routine. */}
          <OffpeakWindowEditor key={routine.id} routine={routine} onUpdated={onUpdated} />
          <div>
            <div className="flex items-center justify-between mb-sm">
              <span className="text-label-sm text-on-surface-variant uppercase tracking-wider">
                {t('tasks.routineDetailDrawer.dependencies')}
              </span>
              <span className="font-label-sm text-label-xs text-on-surface-variant">
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