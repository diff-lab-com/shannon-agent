/**
 * P1-5 C-2 — one panel of the WorkspaceGrid: title bar (icon / title /
 * keyboard menu / maximize / close) + content slot.
 *
 * State-preservation contract (brief: 隐藏面板不卸载 terminal/xterm 与 Live
 * iframe): as long as a panel exists in the layout, its content stays
 * mounted — moving/resizing only rewrites CSS grid placement, hiding
 * (maximize) applies the `hidden` class, never an unmount. Only closing a
 * panel or switching to a preset without it removes it from the tree.
 *
 * Drag model (stable choice, documented in the task report): pointer events
 * for BOTH interactions — title-bar drag swaps with the panel under the
 * pointer (`document.elementFromPoint`), edge/corner handles resize. No
 * HTML5 DnD (its dataTransfer lifecycle is awkward to keep declarative and
 * it does not work with touch). Pointer events unify mouse/touch/pen;
 * handles set `touch-action: none` so touch dragging works too (untested on
 * real devices — documented limitation).
 */
import { useCallback, useRef, useState, type ReactNode, type PointerEvent as ReactPointerEvent } from 'react'
import { useIntl } from 'react-intl'
import { cn } from '@/lib/utils'
import { DropdownMenu, type DropdownMenuItem } from '@/components/ui/dropdown-menu'
import type { MoveDirection, PanelLayout, PanelRect } from './layout'

export type ResizeEdge = 'right' | 'bottom' | 'corner'

interface WorkspacePanelProps {
  panel: PanelLayout
  /** Effective rect — layout rect with maximize/draft overrides applied. */
  rect: PanelRect
  title: string
  icon: string
  /** Title bar + resize handles (false = default focus preset look). */
  chrome: boolean
  /** Layout editing enabled (false in window mode / read-only hosts). */
  editing: boolean
  maximized: boolean
  /** This panel is the pointer drop target during a title-bar drag. */
  dropTarget: boolean
  /** This panel is being dragged (visual dim). */
  dragging: boolean
  /** Another panel is maximized — hide this one but KEEP it mounted. */
  panelHidden: boolean
  onToggleMaximize?: () => void
  /** Undefined for the chat panel — it is not closable. */
  onClose?: () => void
  onMove?: (dir: MoveDirection) => void
  onResizeStart?: (e: ReactPointerEvent<HTMLElement>, panel: PanelLayout, edge: ResizeEdge) => void
  onDragStart?: (panelId: string) => void
  onDragMove?: (x: number, y: number) => void
  onDragEnd?: () => void
  children: ReactNode
}

/** Movement before a press counts as a drag (not a click). */
const DRAG_THRESHOLD_PX = 4

/** Pointer capture keeps drag/resize events flowing outside the element;
 * jsdom and some browsers can refuse it — always best-effort. */
export function capturePointer(e: ReactPointerEvent<HTMLElement>) {
  try {
    e.currentTarget.setPointerCapture?.(e.pointerId)
  } catch { /* capture is an optimization, not a requirement */ }
}

export function WorkspacePanel({
  panel,
  rect,
  title,
  icon,
  chrome,
  editing,
  maximized,
  dropTarget,
  dragging,
  panelHidden,
  onToggleMaximize,
  onClose,
  onMove,
  onResizeStart,
  onDragStart,
  onDragMove,
  onDragEnd,
  children,
}: WorkspacePanelProps) {
  const intl = useIntl()
  const t = useCallback((id: string, values?: Record<string, string>) =>
    intl.formatMessage({ id }, values), [intl])
  const [menuOpen, setMenuOpen] = useState(false)
  const menuButtonRef = useRef<HTMLButtonElement>(null)
  const dragRef = useRef<{ x: number; y: number; active: boolean } | null>(null)

  const startDrag = (e: ReactPointerEvent<HTMLElement>) => {
    if (!editing || e.button !== 0) return
    // Menu button presses must not start a panel swap.
    if ((e.target as HTMLElement).closest('button')) return
    dragRef.current = { x: e.clientX, y: e.clientY, active: false }
    capturePointer(e)
  }

  const moveDrag = (e: ReactPointerEvent<HTMLElement>) => {
    const state = dragRef.current
    if (!state) return
    if (!state.active) {
      if (Math.hypot(e.clientX - state.x, e.clientY - state.y) < DRAG_THRESHOLD_PX) return
      state.active = true
      onDragStart?.(panel.id)
    }
    onDragMove?.(e.clientX, e.clientY)
  }

  const endDrag = () => {
    const state = dragRef.current
    dragRef.current = null
    if (state?.active) onDragEnd?.()
  }

  const moveItem = (dir: MoveDirection, labelId: string): DropdownMenuItem => ({
    id: `move-${dir}`,
    label: t(labelId),
    icon: dir === 'up' ? 'keyboard_arrow_up'
      : dir === 'down' ? 'keyboard_arrow_down'
      : dir === 'left' ? 'keyboard_arrow_left'
      : 'keyboard_arrow_right',
    onSelect: () => onMove?.(dir),
  })

  const menuItems: DropdownMenuItem[] = editing
    ? [
        moveItem('up', 'workspace.panel.moveUp'),
        moveItem('down', 'workspace.panel.moveDown'),
        moveItem('left', 'workspace.panel.moveLeft'),
        moveItem('right', 'workspace.panel.moveRight'),
        ...(onToggleMaximize
          ? [{
              id: 'maximize',
              label: t(maximized ? 'workspace.panel.restore' : 'workspace.panel.maximize'),
              icon: maximized ? 'fullscreen_exit' : 'fullscreen',
              onSelect: () => onToggleMaximize(),
            }]
          : []),
        ...(onClose ? [{ id: 'close', label: t('workspace.panel.close'), icon: 'close', onSelect: () => onClose() }] : []),
      ]
    : []

  return (
    <section
      data-workspace-panel-id={panel.id}
      data-workspace-kind={panel.kind}
      aria-label={title}
      className={cn(
        'relative flex min-h-0 min-w-0 flex-col overflow-hidden',
        chrome && 'rounded-xl border bg-surface-container-lowest',
        chrome && (dropTarget ? 'border-primary ring-2 ring-primary/40' : 'border-outline-variant/30'),
        dragging && 'opacity-50',
        panelHidden && 'hidden',
        maximized && 'z-raised shadow-[var(--shadow-e3)]',
      )}
      style={{
        gridColumn: `${rect.col} / span ${rect.w}`,
        gridRow: `${rect.row} / span ${rect.h}`,
      }}
    >
      {chrome && (
        <header
          data-workspace-drag-handle
          onPointerDown={startDrag}
          onPointerMove={moveDrag}
          onPointerUp={endDrag}
          onPointerCancel={endDrag}
          className={cn(
            'flex shrink-0 items-center gap-xs border-b border-outline-variant/20 bg-surface-container-low px-sm py-1',
            editing ? 'cursor-grab active:cursor-grabbing touch-none select-none' : '',
          )}
        >
          <span className="material-symbols-outlined icon-sm text-primary shrink-0" aria-hidden="true">{icon}</span>
          <span className="font-label-md text-label-md text-on-surface truncate">{title}</span>
          <span className="flex-1 min-w-0" />
          {editing && menuItems.length > 0 && (
            <span className="relative">
              <button
                ref={menuButtonRef}
                type="button"
                data-panel-focus-target
                aria-haspopup="menu"
                aria-expanded={menuOpen}
                aria-label={t('workspace.panel.menu.aria', { title })}
                onClick={() => setMenuOpen(open => !open)}
                className="p-0.5 rounded text-on-surface-variant hover:text-on-surface hover:bg-surface-container focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
              >
                <span className="material-symbols-outlined icon-sm" aria-hidden="true">more_vert</span>
              </button>
              <DropdownMenu
                open={menuOpen}
                onClose={() => setMenuOpen(false)}
                items={menuItems}
                triggerRef={menuButtonRef}
                ariaLabel={t('workspace.panel.menu.aria', { title })}
              />
            </span>
          )}
        </header>
      )}

      <div className="relative flex-1 min-h-0 min-w-0 overflow-hidden">
        {children}
      </div>

      {chrome && editing && onResizeStart && (
        <>
          <div
            data-resize-edge="right"
            role="separator"
            aria-label={t('workspace.panel.resizeX.aria')}
            aria-orientation="vertical"
            onPointerDown={(e) => onResizeStart(e, panel, 'right')}
            className="absolute top-0 right-0 bottom-0 w-1.5 cursor-col-resize touch-none hover:bg-primary/30 z-raised"
          />
          <div
            data-resize-edge="bottom"
            role="separator"
            aria-label={t('workspace.panel.resizeY.aria')}
            aria-orientation="horizontal"
            onPointerDown={(e) => onResizeStart(e, panel, 'bottom')}
            className="absolute left-0 right-0 bottom-0 h-1.5 cursor-row-resize touch-none hover:bg-primary/30 z-raised"
          />
          <div
            data-resize-edge="corner"
            role="separator"
            aria-label={t('workspace.panel.resize.aria')}
            onPointerDown={(e) => onResizeStart(e, panel, 'corner')}
            className="absolute right-0 bottom-0 size-3 cursor-nwse-resize touch-none hover:bg-primary/40 z-raised"
          />
        </>
      )}
    </section>
  )
}
