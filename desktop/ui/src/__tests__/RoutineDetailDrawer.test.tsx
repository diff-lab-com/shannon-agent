import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, fireEvent, act } from '@testing-library/react'
import RoutineDetailDrawer from '@/components/tasks/RoutineDetailDrawer'
import type { ScheduledRoutine } from '@/types'

vi.mock('@/components/tasks/DependsOnEditor', () => ({
  default: () => <div data-testid="deps-editor">DependsOnEditor</div>,
}))

function makeRoutine(overrides: Partial<ScheduledRoutine> = {}): ScheduledRoutine {
  return {
    id: 'r1',
    name: 'Daily Standup',
    prompt: 'Summarize today',
    interval_secs: 86400,
    trigger_type: 'interval',
    enabled: true,
    created_at: 1717000000,
    next_fire_at: 1717000060,
    fire_count: 5,
    ...overrides,
  }
}

describe('RoutineDetailDrawer', () => {
  it('renders nothing when routine is null', () => {
    const { container } = render(
      <RoutineDetailDrawer routine={null} routines={[]} onClose={() => {}} />,
    )
    expect(container).toBeEmptyDOMElement()
  })

  it('renders routine name and prompt as editable basics (P1-1)', () => {
    render(<RoutineDetailDrawer routine={makeRoutine()} routines={[]} onClose={() => {}} />)
    // P1-1: name/prompt moved into the RoutineBasicsEditor inputs.
    expect(screen.getByDisplayValue('Daily Standup')).toBeInTheDocument()
    expect(screen.getByDisplayValue('Summarize today')).toBeInTheDocument()
  })

  it('shows trigger type capitalized', () => {
    render(<RoutineDetailDrawer routine={makeRoutine()} routines={[]} onClose={() => {}} />)
    // Both the basics editor radio and the info grid render the trigger.
    expect(screen.getAllByText('Interval').length).toBeGreaterThan(0)
  })

  it('shows Enabled as Yes when enabled true', () => {
    render(<RoutineDetailDrawer routine={makeRoutine()} routines={[]} onClose={() => {}} />)
    expect(screen.getByText('Yes')).toBeInTheDocument()
  })

  it('shows last_error block when routine has error', () => {
    render(
      <RoutineDetailDrawer
        routine={makeRoutine({ last_error: 'Boom' })}
        routines={[]}
        onClose={() => {}}
      />,
    )
    expect(screen.getByText('Boom')).toBeInTheDocument()
  })

  it('renders DependsOnEditor', () => {
    render(<RoutineDetailDrawer routine={makeRoutine()} routines={[]} onClose={() => {}} />)
    expect(screen.getByTestId('deps-editor')).toBeInTheDocument()
  })

  it('shows resolved dep names instead of IDs when deps present', () => {
    render(
      <RoutineDetailDrawer
        routine={makeRoutine({ depends_on: ['r2'] })}
        routines={[makeRoutine(), makeRoutine({ id: 'r2', name: 'Deploy' })]}
        onClose={() => {}}
      />,
    )
    expect(screen.getByText('Deploy')).toBeInTheDocument()
  })

  it('shows None when no deps', () => {
    render(<RoutineDetailDrawer routine={makeRoutine()} routines={[]} onClose={() => {}} />)
    expect(screen.getByText('None')).toBeInTheDocument()
  })

  it('calls onClose when close button clicked', () => {
    const onClose = vi.fn()
    render(<RoutineDetailDrawer routine={makeRoutine()} routines={[]} onClose={onClose} />)
    fireEvent.click(screen.getByRole('button', { name: /Close drawer/i }))
    expect(onClose).toHaveBeenCalled()
  })

  // Backdrop dismissal is now delegated to Base UI's useDismiss layer,
  // which doesn't fire reliably from `fireEvent.click` in jsdom (its
  // document-level capture-phase listener relies on composedPath, which
  // jsdom doesn't simulate for portal-rendered subtrees).
  // Backdrop-click is covered by `e2e/modals.spec.ts` (R5) in real Chromium.
  it.skip('calls onClose when backdrop clicked (covered by R5 e2e)', () => {})

  it('does not call onClose when inner panel clicked', () => {
    const onClose = vi.fn()
    render(<RoutineDetailDrawer routine={makeRoutine()} routines={[]} onClose={onClose} />)
    fireEvent.click(screen.getByDisplayValue('Daily Standup'))
    expect(onClose).not.toHaveBeenCalled()
  })

  // P1-1: lifecycle row (pause switch + delete) is embedded in the drawer.
  it('renders the lifecycle toggle and delete controls', () => {
    render(<RoutineDetailDrawer routine={makeRoutine()} routines={[]} onClose={() => {}} />)
    expect(screen.getByTestId('routine-lifecycle-toggle')).toBeInTheDocument()
    expect(screen.getByTestId('routine-lifecycle-delete')).toBeInTheDocument()
  })

  // P2-5: the off-peak execution window editor is embedded in the drawer.
  it('shows the off-peak editor with the window badge when configured', () => {
    const routine = makeRoutine({
      policy: {
        max_retries: 0,
        timeout_secs: 0,
        worktree: null,
        notify_on_failure: false,
        budget_usd: null,
        auto_archive_when_empty: true,
        execution_window: { start_hour: 22, end_hour: 6, timezone: null },
      },
    })
    render(<RoutineDetailDrawer routine={routine} routines={[]} onClose={() => {}} />)
    expect(screen.getByText('Off-peak execution')).toBeInTheDocument()
    expect(screen.getByText('Off-peak 22:00–06:00')).toBeInTheDocument()
    // No queued run in the (empty) history → no queued status line.
    expect(screen.queryByText(/Queued \(off-peak/)).not.toBeInTheDocument()
  })
})

// ── 补3 (design-parity R2 §7.1): the 「查看日志」 error anchor — the drawer
// opens from Triage's failed card, scrolls to its last_error block and
// flashes a one-shot ring (~2s). jsdom seam: scrollIntoView (stubbed in the
// global setup) is asserted as CALLS, the flash as class/attribute. ────────
describe('RoutineDetailDrawer — focusLastError anchor (补3)', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
    vi.useRealTimers()
  })

  function stubRafSync() {
    // Run requestAnimationFrame callbacks synchronously so the anchor effect
    // is deterministic under fake timers (vitest's default toFake list does
    // not include rAF).
    vi.stubGlobal('requestAnimationFrame', (cb: FrameRequestCallback) => {
      cb(0)
      return 0
    })
  }

  function spyScrollIntoView() {
    const proto = Element.prototype as unknown as Record<string, unknown>
    const prev = proto.scrollIntoView
    const spy = vi.fn()
    proto.scrollIntoView = spy
    return { spy, restore: () => { proto.scrollIntoView = prev } }
  }

  it('scrolls the last_error block into view and rings it for ~2s', () => {
    stubRafSync()
    vi.useFakeTimers()
    const { spy, restore } = spyScrollIntoView()
    try {
      render(
        <RoutineDetailDrawer
          routine={makeRoutine({ last_error: 'Boom' })}
          routines={[]}
          onClose={() => {}}
          focusLastError
        />,
      )
      const block = screen.getByTestId('routine-last-error')
      // The one-shot ring + the scrolled-to target are both on the block.
      expect(block).toHaveAttribute('data-error-flash', 'true')
      expect(block.className).toContain('ring-2')
      expect(spy).toHaveBeenCalledTimes(1)
      expect(spy.mock.instances[0]).toBe(block)
      // One-shot: the ring fades after ~2s, the block stays.
      act(() => { vi.advanceTimersByTime(2000) })
      expect(block).not.toHaveAttribute('data-error-flash')
      expect(block.className).not.toContain('ring-2')
      expect(screen.getByTestId('routine-last-error')).toHaveTextContent('Boom')
    } finally {
      restore()
    }
  })

  it('does not ring or scroll without the focusLastError flag', () => {
    stubRafSync()
    vi.useFakeTimers()
    const { spy, restore } = spyScrollIntoView()
    try {
      render(
        <RoutineDetailDrawer
          routine={makeRoutine({ last_error: 'Boom' })}
          routines={[]}
          onClose={() => {}}
        />,
      )
      expect(screen.getByTestId('routine-last-error')).not.toHaveAttribute('data-error-flash')
      expect(spy).not.toHaveBeenCalled()
    } finally {
      restore()
    }
  })

  it('does not ring when the routine has no last_error to anchor', () => {
    stubRafSync()
    vi.useFakeTimers()
    const { spy, restore } = spyScrollIntoView()
    try {
      render(
        <RoutineDetailDrawer
          routine={makeRoutine()}
          routines={[]}
          onClose={() => {}}
          focusLastError
        />,
      )
      expect(screen.queryByTestId('routine-last-error')).not.toBeInTheDocument()
      expect(spy).not.toHaveBeenCalled()
    } finally {
      restore()
    }
  })

  it('clears the ring as soon as the hand-over flag turns false (drawer closed)', () => {
    stubRafSync()
    vi.useFakeTimers()
    const { restore } = spyScrollIntoView()
    try {
      const props = {
        routine: makeRoutine({ last_error: 'Boom' }),
        routines: [],
        onClose: () => {},
      }
      const { rerender } = render(<RoutineDetailDrawer {...props} focusLastError />)
      expect(screen.getByTestId('routine-last-error')).toHaveAttribute('data-error-flash', 'true')
      // Esc / close flips the caller's flag — the ring must drop immediately,
      // not linger until the 2s timer.
      rerender(<RoutineDetailDrawer {...props} focusLastError={false} />)
      expect(screen.getByTestId('routine-last-error')).not.toHaveAttribute('data-error-flash')
    } finally {
      restore()
    }
  })
})
