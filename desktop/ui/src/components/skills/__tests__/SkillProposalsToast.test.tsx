// Tests for the degraded skill-proposals toast (IA X1): a light nudge that
// links to /extensions/pending — no review panel behind it anymore.
//
// X1 fix coverage: the toast listens to the backend's
// `skill-proposal-available` event (draft arrivals) again, and stays hidden
// on /extensions/pending itself.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, act } from '@testing-library/react'
import { MemoryRouter, useLocation } from 'react-router-dom'
import SkillProposalsToast from '../SkillProposalsToast'

const hookSpy = vi.hoisted(() => vi.fn())
const eventSpy = vi.hoisted(() => vi.fn())

vi.mock('@/hooks/usePendingSkillCandidates', () => ({
  usePendingSkillCandidates: () => hookSpy(),
}))

vi.mock('@/hooks/useTauriEventValidated', () => ({
  useTauriEventValidated: (...args: unknown[]) => eventSpy(...args),
}))

// Latest handler registered for 'skill-proposal-available'.
let proposalEventHandler: ((event: { payload: { pending_count: number } }) => void) | null = null

function emitProposalEvent(pendingCount: number) {
  act(() => {
    proposalEventHandler?.({ payload: { pending_count: pendingCount } })
  })
}

function LocationCapture() {
  const location = useLocation()
  return <div data-testid="location">{location.pathname}</div>
}

function renderToast(candidates: unknown[] = [], path = '/') {
  const refetch = vi.fn()
  hookSpy.mockReturnValue({ candidates, loading: false, refetch })
  render(
    <MemoryRouter initialEntries={[path]}>
      <SkillProposalsToast />
      <LocationCapture />
    </MemoryRouter>,
  )
  return { refetch }
}

beforeEach(() => {
  hookSpy.mockReset()
  eventSpy.mockReset()
  proposalEventHandler = null
  eventSpy.mockImplementation((_event: string, handler: (e: { payload: { pending_count: number } }) => void) => {
    proposalEventHandler = handler
  })
})

describe('SkillProposalsToast (light nudge)', () => {
  it('renders nothing when nothing is pending', () => {
    renderToast([])
    expect(screen.queryByText(/skill suggestion/i)).not.toBeInTheDocument()
  })

  it('renders the pending count when candidates exist', () => {
    renderToast([{ id: 'a' }, { id: 'b' }])
    expect(screen.getByText('2 skill suggestions')).toBeInTheDocument()
  })

  it('navigates to /extensions/pending on Review click (no panel opens)', () => {
    renderToast([{ id: 'a' }])
    fireEvent.click(screen.getByText('Review'))
    expect(screen.getByTestId('location')).toHaveTextContent('/extensions/pending')
  })

  it('Close hides the nudge without navigation', () => {
    renderToast([{ id: 'a' }])
    fireEvent.click(screen.getByRole('button', { name: 'Close' }))
    expect(screen.queryByText(/skill suggestion/i)).not.toBeInTheDocument()
    expect(screen.getByTestId('location')).toHaveTextContent('/')
  })

  // X1 fix: draft arrivals (`skill-proposal-available`) surface as the same
  // light nudge — the toast is the only global arrival signal for drafts.
  it('shows the draft nudge when skill-proposal-available fires', () => {
    renderToast([])
    expect(proposalEventHandler).not.toBeNull()
    expect(screen.queryByText(/skill suggestion/i)).not.toBeInTheDocument()
    emitProposalEvent(3)
    expect(screen.getByText('3 skill suggestions')).toBeInTheDocument()
  })

  it('clears the draft nudge when the count returns to zero', () => {
    renderToast([])
    emitProposalEvent(2)
    expect(screen.getByText('2 skill suggestions')).toBeInTheDocument()
    // Backend re-emits with the updated count after approve/reject.
    emitProposalEvent(0)
    expect(screen.queryByText(/skill suggestion/i)).not.toBeInTheDocument()
  })

  // X1 评审 Minor #1: on the pending page itself the nudge is noise — the
  // queue updates in place there.
  it('stays hidden on /extensions/pending even with pending items', () => {
    renderToast([{ id: 'a' }], '/extensions/pending')
    emitProposalEvent(2)
    expect(screen.queryByText(/skill suggestion/i)).not.toBeInTheDocument()
    expect(screen.getByTestId('location')).toHaveTextContent('/extensions/pending')
  })

  it('re-arms after dismissal when the count changes', () => {
    hookSpy.mockReturnValue({ candidates: [{ id: 'a' }], loading: false, refetch: vi.fn() })
    const { rerender } = render(
      <MemoryRouter>
        <SkillProposalsToast />
        <LocationCapture />
      </MemoryRouter>,
    )
    expect(screen.getByText('1 skill suggestion')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Close' }))
    expect(screen.queryByText(/skill suggestion/i)).not.toBeInTheDocument()

    act(() => hookSpy.mockReturnValue({ candidates: [{ id: 'a' }, { id: 'b' }], loading: false, refetch: vi.fn() }))
    rerender(
      <MemoryRouter>
        <SkillProposalsToast />
        <LocationCapture />
      </MemoryRouter>,
    )
    expect(screen.getByText('2 skill suggestions')).toBeInTheDocument()
  })
})
