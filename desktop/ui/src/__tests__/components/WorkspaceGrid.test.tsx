// P1-5 C-2 — WorkspaceGrid behaviour tests: rendering, lazy-mount state
// preservation, pointer drag-swap, resize, keyboard menu moves, F6 cycling,
// maximize, and the read-only (editing disabled) mode.

import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { useEffect } from 'react'
import { WorkspaceGrid } from '@/components/workspace/WorkspaceGrid'
import { movePanel, presetLayout, swapPanels, type PanelKind, type PanelLayout, type WorkspaceLayout } from '@/components/workspace/layout'

let mountCount = 0
// Probe content: counts MOUNTS (not renders) so tests can assert that layout
// edits never remount panel content — the terminal/xterm + live-iframe
// state-preservation contract.
function Probe() {
  useEffect(() => { mountCount += 1 }, [])
  return <div data-testid="probe" />
}

/** jsdom has no elementFromPoint; hand it a fixed target for drag tests. */
function stubElementFromPoint(target: Element): () => void {
  const original = document.elementFromPoint
  document.elementFromPoint = () => target
  return () => { document.elementFromPoint = original }
}

function renderGrid(layout: WorkspaceLayout) {
  const onChangeSpy = vi.fn((next: WorkspaceLayout) => setLayout(next))
  let current = layout
  const setLayout = (next: WorkspaceLayout) => { current = next }

  function GridHost({ initial }: { initial: WorkspaceLayout }) {
    return (
      <div style={{ width: 1200, height: 1200 }}>
        <WorkspaceGrid
          layout={initial}
          chrome
          onChange={onChangeSpy}
          renderPanelContent={(kind: PanelKind, _panel: PanelLayout) => <Probe key={kind} />}
        />
      </div>
    )
  }

  const view = render(<GridHost initial={current} />)
  return {
    onChangeSpy,
    /** Apply a new layout the way a host would and rerender. */
    applyWith: (next: WorkspaceLayout) => {
      setLayout(next)
      view.rerender(<GridHost initial={current} />)
    },
  }
}

beforeEach(() => {
  mountCount = 0
})

afterEach(() => {
  vi.restoreAllMocks()
})

describe('WorkspaceGrid', () => {
  it('renders one section per panel with kind titles (a11y labels)', () => {
    renderGrid(presetLayout('review'))
    expect(screen.getByTestId('workspace-grid')).toBeInTheDocument()
    expect(screen.getByRole('region', { name: 'Chat' })).toBeInTheDocument()
    expect(screen.getByRole('region', { name: 'Diff' })).toBeInTheDocument()
    expect(screen.getAllByTestId('probe')).toHaveLength(2)
  })

  it('keeps panel content mounted across a layout edit (state preservation)', () => {
    const review = presetLayout('review')
    const { applyWith } = renderGrid(review)
    expect(mountCount).toBe(2)

    // Swap the panels (a full layout edit) — the probes must NOT remount:
    // the same contract that keeps xterm + the live iframe alive.
    applyWith(swapPanels(review, 'chat', 'diff'))
    expect(mountCount).toBe(2)
    expect(screen.getByRole('region', { name: 'Chat' }).style.gridColumn).toBe('9 / span 4')
  })

  it('pointer drag on a title bar swaps the two panels', () => {
    const { onChangeSpy } = renderGrid(presetLayout('review'))
    const chatHeader = screen.getByRole('region', { name: 'Chat' })
      .querySelector('[data-workspace-drag-handle]')!
    const diffPanel = screen.getByRole('region', { name: 'Diff' })
    const restore = stubElementFromPoint(diffPanel)
    try {
      fireEvent.pointerDown(chatHeader, { button: 0, clientX: 10, clientY: 10 })
      fireEvent.pointerMove(chatHeader, { clientX: 60, clientY: 12 })
      fireEvent.pointerUp(chatHeader)
    } finally {
      restore()
    }

    expect(onChangeSpy).toHaveBeenCalledTimes(1)
    const next = onChangeSpy.mock.calls[0][0] as WorkspaceLayout
    // Rects exchanged, ids stay put.
    const review = presetLayout('review')
    expect(next.panels.find(p => p.id === 'chat')?.rect).toEqual(review.panels.find(p => p.id === 'diff')?.rect)
    expect(next.panels.find(p => p.id === 'diff')?.rect).toEqual(review.panels.find(p => p.id === 'chat')?.rect)
  })

  it('pointer drag below the threshold does not swap', () => {
    const { onChangeSpy } = renderGrid(presetLayout('review'))
    const chatHeader = screen.getByRole('region', { name: 'Chat' })
      .querySelector('[data-workspace-drag-handle]')!
    const restore = stubElementFromPoint(screen.getByRole('region', { name: 'Diff' }))
    try {
      fireEvent.pointerDown(chatHeader, { button: 0, clientX: 10, clientY: 10 })
      fireEvent.pointerMove(chatHeader, { clientX: 12, clientY: 10 }) // < 4px threshold
      fireEvent.pointerUp(chatHeader)
    } finally {
      restore()
    }
    expect(onChangeSpy).not.toHaveBeenCalled()
  })

  it('resize handle drags commit a collision-safe rect', () => {
    const rectSpy = vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect')
      .mockReturnValue({ width: 1200, height: 1200, x: 0, y: 0, top: 0, left: 0, right: 1200, bottom: 1200, toJSON: () => ({}) } as DOMRect)
    const { onChangeSpy } = renderGrid(presetLayout('review'))

    // Grid cell = 100px. Growing the chat panel (w 8 → 10) would collide
    // with the diff panel (cols 9-12): the commit must clamp back to w 8.
    const chatHandle = screen.getByRole('region', { name: 'Chat' })
      .querySelector('[data-resize-edge="right"]')!
    fireEvent.pointerDown(chatHandle, { button: 0, clientX: 800, clientY: 10 })
    fireEvent(window, new MouseEvent('pointermove', { clientX: 1000, clientY: 10 }))
    fireEvent(window, new MouseEvent('pointerup', { clientX: 1000, clientY: 10 }))

    expect(onChangeSpy).toHaveBeenCalledTimes(1)
    const next = onChangeSpy.mock.calls[0][0] as WorkspaceLayout
    expect(next.panels.find(p => p.id === 'chat')?.rect.w).toBe(8)
    expect(rectSpy).toHaveBeenCalled()
  })

  it('resize shrinks freely when space is available', () => {
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect')
      .mockReturnValue({ width: 1200, height: 1200, x: 0, y: 0, top: 0, left: 0, right: 1200, bottom: 1200, toJSON: () => ({}) } as DOMRect)
    const { onChangeSpy } = renderGrid(presetLayout('review'))

    const diffHandle = screen.getByRole('region', { name: 'Diff' })
      .querySelector('[data-resize-edge="bottom"]')!
    fireEvent.pointerDown(diffHandle, { button: 0, clientX: 1000, clientY: 1200 })
    fireEvent(window, new MouseEvent('pointermove', { clientX: 1000, clientY: 900 }))
    fireEvent(window, new MouseEvent('pointerup', { clientX: 1000, clientY: 900 }))

    const next = onChangeSpy.mock.calls[0][0] as WorkspaceLayout
    expect(next.panels.find(p => p.id === 'diff')?.rect.h).toBe(9) // 12 − 3 cells
  })

  it('keyboard menu move (上移/move up equivalent) edits the layout', () => {
    const { onChangeSpy } = renderGrid(presetLayout('review'))
    fireEvent.click(screen.getByRole('button', { name: 'Panel menu: Diff' }))
    fireEvent.click(screen.getByRole('menuitem', { name: 'Move up' }))
    expect(onChangeSpy).toHaveBeenCalledTimes(1)
    const next = onChangeSpy.mock.calls[0][0] as WorkspaceLayout
    // The diff panel is full-height: up is blocked, so the committed layout
    // equals movePanel's no-op result (same rects).
    expect(next).toEqual(movePanel(presetLayout('review'), 'diff', 'up'))
  })

  it('F6 cycles focus across panel title bars', () => {
    renderGrid(presetLayout('review'))
    const chatMenu = screen.getByRole('button', { name: 'Panel menu: Chat' })
    const diffMenu = screen.getByRole('button', { name: 'Panel menu: Diff' })
    chatMenu.focus()
    expect(document.activeElement).toBe(chatMenu)
    fireEvent.keyDown(screen.getByTestId('workspace-grid'), { key: 'F6' })
    expect(document.activeElement).toBe(diffMenu)
    fireEvent.keyDown(screen.getByTestId('workspace-grid'), { key: 'F6', shiftKey: true })
    expect(document.activeElement).toBe(chatMenu)
  })

  it('maximize hides the other panel but keeps it mounted', () => {
    renderGrid(presetLayout('review'))
    const hasHiddenClass = (name: string) =>
      screen.getByRole('region', { name }).className.split(/\s+/).includes('hidden')

    fireEvent.click(screen.getByRole('button', { name: 'Panel menu: Chat' }))
    fireEvent.click(screen.getByRole('menuitem', { name: 'Maximize panel' }))
    expect(hasHiddenClass('Diff')).toBe(true)
    // Both probes still mounted — the hidden panel kept its state.
    expect(screen.getAllByTestId('probe')).toHaveLength(2)

    fireEvent.click(screen.getByRole('button', { name: 'Panel menu: Chat' }))
    fireEvent.click(screen.getByRole('menuitem', { name: 'Restore panel size' }))
    expect(hasHiddenClass('Diff')).toBe(false)
  })

  it('editing disabled renders no menus or resize handles', () => {
    render(
      <WorkspaceGrid
        layout={presetLayout('review')}
        chrome
        renderPanelContent={kind => <Probe key={kind} />}
      />,
    )
    expect(screen.queryByRole('button', { name: /Panel menu/ })).toBeNull()
    expect(document.querySelectorAll('[data-resize-edge]')).toHaveLength(0)
    expect(screen.getByTestId('workspace-grid')).toHaveAttribute('aria-disabled', 'true')
  })

  it('chrome=false renders no title bars or handles (default focus look)', () => {
    render(
      <WorkspaceGrid
        layout={presetLayout('focus')}
        chrome={false}
        onChange={() => {}}
        renderPanelContent={() => <Probe />}
      />,
    )
    expect(document.querySelectorAll('[data-workspace-drag-handle]')).toHaveLength(0)
    expect(document.querySelectorAll('[data-resize-edge]')).toHaveLength(0)
    expect(screen.getAllByTestId('probe')).toHaveLength(1)
  })
})
