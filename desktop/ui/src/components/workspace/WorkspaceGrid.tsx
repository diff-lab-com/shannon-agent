/**
 * P1-5 C-2 — the panel grid: a 12-column CSS grid (12 equal `1fr` rows)
 * that places WorkspacePanels by their layout rects and hosts the editing
 * interactions (pointer drag-swap, edge resize, maximize, keyboard access).
 *
 * Zero new dependencies (brief): grid + DnD are self-implemented with
 * pointer events; geometry math lives in ./layout.ts.
 *
 * State preservation: panel content is keyed by the stable panel id, so
 * moving/resizing/hiding never remounts it — xterm instances and the live
 * preview iframe keep running across layout edits (asserted in tests).
 *
 * Keyboard accessibility (brief): F6 / Shift+F6 cycles focus across panel
 * title bars; the per-panel menu offers 上移/下移/左移/右移 (move up/down/
 * left/right), maximize and close as the keyboard-equivalent of dragging.
 */
import { useCallback, useEffect, useRef, useState, type ReactNode, type PointerEvent as ReactPointerEvent } from 'react'
import { useIntl } from 'react-intl'
import { cn } from '@/lib/utils'
import {
  GRID_COLUMNS,
  GRID_ROWS,
  findPanel,
  movePanel,
  removePanel,
  resizePanel,
  swapPanels,
  type MoveDirection,
  type PanelKind,
  type PanelLayout,
  type PanelRect,
  type WorkspaceLayout,
} from './layout'
import { WorkspacePanel, capturePointer, type ResizeEdge } from './WorkspacePanel'

interface WorkspaceGridProps {
  layout: WorkspaceLayout
  /** Render the content of one panel (chat/diff/preview/terminal). */
  renderPanelContent: (kind: PanelKind, panel: PanelLayout) => ReactNode
  /** Undefined ⇒ editing disabled (read-only host, e.g. window mode). */
  onChange?: (next: WorkspaceLayout) => void
  /** Title bars, gaps, resize handles. False = default focus look. */
  chrome: boolean
  ariaLabel?: string
}

const KIND_ICON: Record<PanelKind, string> = {
  chat: 'chat',
  diff: 'difference',
  preview: 'web',
  terminal: 'terminal',
}

interface DragState {
  sourceId: string
  targetId: string | null
}

interface ResizeState {
  id: string
  base: PanelRect
  startX: number
  startY: number
  cellW: number
  cellH: number
  edge: ResizeEdge
}

export function WorkspaceGrid({ layout, renderPanelContent, onChange, chrome, ariaLabel }: WorkspaceGridProps) {
  const intl = useIntl()
  const t = useCallback((id: string, values?: Record<string, string>) =>
    intl.formatMessage({ id }, values), [intl])
  const gridRef = useRef<HTMLDivElement>(null)
  const [maximizedId, setMaximizedId] = useState<string | null>(null)
  const [drag, setDrag] = useState<DragState | null>(null)
  const [draft, setDraft] = useState<{ id: string; rect: PanelRect } | null>(null)
  const resizeRef = useRef<ResizeState | null>(null)

  const editing = onChange !== undefined

  // A maximized/hidden panel that leaves the layout must not stay pinned.
  useEffect(() => {
    if (maximizedId && !findPanel(layout, maximizedId)) setMaximizedId(null)
  }, [layout, maximizedId])

  // ── Editing interactions ────────────────────────────────────────────

  const handleMove = useCallback((id: string, dir: MoveDirection) => {
    const panel = findPanel(layout, id)
    if (!panel) return
    onChange?.(movePanel(layout, id, dir))
  }, [layout, onChange])

  const handleDragStart = useCallback((panelId: string) => {
    setDrag({ sourceId: panelId, targetId: null })
  }, [])

  const handleDragMove = useCallback((x: number, y: number) => {
    setDrag(prev => {
      if (!prev) return prev
      const el = document.elementFromPoint(x, y)?.closest('[data-workspace-panel-id]')
      const targetId = el?.getAttribute('data-workspace-panel-id') ?? null
      const next = targetId && targetId !== prev.sourceId ? targetId : null
      return prev.targetId === next ? prev : { ...prev, targetId: next }
    })
  }, [])

  const handleDragEnd = useCallback(() => {
    setDrag(prev => {
      if (prev?.targetId) onChange?.(swapPanels(layout, prev.sourceId, prev.targetId))
      return null
    })
  }, [layout, onChange])

  const handleResizeStart = useCallback((e: ReactPointerEvent<HTMLElement>, panel: PanelLayout, edge: ResizeEdge) => {
    if (!editing) return
    const gridRect = gridRef.current?.getBoundingClientRect()
    if (!gridRect || gridRect.width === 0 || gridRect.height === 0) return
    e.preventDefault()
    e.stopPropagation()
    resizeRef.current = {
      id: panel.id,
      base: panel.rect,
      startX: e.clientX,
      startY: e.clientY,
      cellW: gridRect.width / GRID_COLUMNS,
      cellH: gridRect.height / GRID_ROWS,
      edge,
    }
    // Preview immediately so the handles' base panel shows the live rect.
    setDraft({ id: panel.id, rect: panel.rect })
    capturePointer(e)
  }, [editing])

  const handleResizeMove = useCallback((x: number, y: number) => {
    const state = resizeRef.current
    if (!state) return
    const dw = Math.round((x - state.startX) / state.cellW)
    const dh = Math.round((y - state.startY) / state.cellH)
    setDraft({
      id: state.id,
      rect: {
        col: state.base.col,
        row: state.base.row,
        w: Math.max(1, state.base.w + (state.edge !== 'bottom' ? dw : 0)),
        h: Math.max(1, state.base.h + (state.edge !== 'right' ? dh : 0)),
      },
    })
  }, [])

  const handleResizeEnd = useCallback(() => {
    resizeRef.current = null
    setDraft(prev => {
      if (prev) onChange?.(resizePanel(layout, prev.id, prev.rect))
      return null
    })
  }, [layout, onChange])

  // Resize tracking rides window-level pointer listeners: the handle sets
  // pointer capture, so move/up events retarget to it and bubble to window.
  // (This also keeps WorkspacePanel free of resize plumbing.)
  useEffect(() => {
    if (!draft) return
    const onMove = (e: PointerEvent) => handleResizeMove(e.clientX, e.clientY)
    const onUp = () => handleResizeEnd()
    window.addEventListener('pointermove', onMove)
    window.addEventListener('pointerup', onUp)
    return () => {
      window.removeEventListener('pointermove', onMove)
      window.removeEventListener('pointerup', onUp)
    }
  }, [draft, handleResizeMove, handleResizeEnd])

  // ── Keyboard accessibility (F6 focus cycling) ───────────────────────

  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (e.key !== 'F6' || !gridRef.current) return
    e.preventDefault()
    const targets = Array.from(
      gridRef.current.querySelectorAll<HTMLElement>('[data-panel-focus-target]'),
    )
    if (targets.length === 0) return
    const current = targets.indexOf(document.activeElement as HTMLElement)
    const step = e.shiftKey ? -1 : 1
    const next = targets[(current + step + targets.length) % targets.length]
    next.focus()
  }, [])

  const effectiveRect = (panel: PanelLayout): PanelRect => {
    if (maximizedId === panel.id) return { col: 1, row: 1, w: GRID_COLUMNS, h: GRID_ROWS }
    if (draft?.id === panel.id) return draft.rect
    return panel.rect
  }

  return (
    <div
      ref={gridRef}
      role="region"
      aria-label={ariaLabel ?? t('workspace.grid.aria')}
      aria-disabled={!editing}
      data-testid="workspace-grid"
      onKeyDown={handleKeyDown}
      className={cn('grid h-full w-full min-h-0 overflow-hidden', chrome && editing ? 'gap-xs p-xs' : 'gap-0')}
      style={{
        gridTemplateColumns: `repeat(${GRID_COLUMNS}, minmax(0, 1fr))`,
        gridTemplateRows: `repeat(${GRID_ROWS}, minmax(0, 1fr))`,
      }}
    >
      {layout.panels.map(panel => (
        <WorkspacePanel
          key={panel.id}
          panel={panel}
          rect={effectiveRect(panel)}
          title={t(`workspace.panel.${panel.kind}`)}
          icon={KIND_ICON[panel.kind]}
          chrome={chrome}
          editing={editing}
          maximized={maximizedId === panel.id}
          dropTarget={drag?.targetId === panel.id}
          dragging={drag?.sourceId === panel.id}
          panelHidden={maximizedId !== null && maximizedId !== panel.id}
          onToggleMaximize={chrome && editing
            ? () => setMaximizedId(id => (id === panel.id ? null : panel.id))
            : undefined}
          onClose={panel.kind !== 'chat' && onChange
            ? () => onChange(removePanel(layout, panel.id))
            : undefined}
          onMove={chrome && editing ? (dir) => handleMove(panel.id, dir) : undefined}
          onResizeStart={chrome && editing ? handleResizeStart : undefined}
          onDragStart={chrome && editing ? handleDragStart : undefined}
          onDragMove={chrome && editing ? handleDragMove : undefined}
          onDragEnd={chrome && editing ? handleDragEnd : undefined}
        >
          {renderPanelContent(panel.kind, panel)}
        </WorkspacePanel>
      ))}
    </div>
  )
}
