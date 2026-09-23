// Tests for the degraded skill-proposals toast (IA X1): a light nudge that
// links to /extensions/pending — no review panel behind it anymore.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, act } from '@testing-library/react'
import { MemoryRouter, useLocation } from 'react-router-dom'
import SkillProposalsToast from '../SkillProposalsToast'

const hookSpy = vi.hoisted(() => vi.fn())

vi.mock('@/hooks/usePendingSkillCandidates', () => ({
  usePendingSkillCandidates: () => hookSpy(),
}))

function LocationCapture() {
  const location = useLocation()
  return <div data-testid="location">{location.pathname}</div>
}

function renderToast(candidates: unknown[] = []) {
  const refetch = vi.fn()
  hookSpy.mockReturnValue({ candidates, loading: false, refetch })
  render(
    <MemoryRouter>
      <SkillProposalsToast />
      <LocationCapture />
    </MemoryRouter>,
  )
  return { refetch }
}

beforeEach(() => {
  hookSpy.mockReset()
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
