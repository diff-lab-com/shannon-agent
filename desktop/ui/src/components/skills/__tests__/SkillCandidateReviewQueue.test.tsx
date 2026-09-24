// Tests for the Extensions → Pending skill-candidate review queue (IA X1):
// renders the queue, the approve flow (through SkillApprovalModal), the
// direct reject flow, and the one-shot focus handed over by the inbox
// card's 「去审查」 jump.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor, within } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import SkillCandidateReviewQueue from '../SkillCandidateReviewQueue'
import * as api from '@/lib/tauri-api'
import type { SkillCandidate } from '@/lib/tauri-api'

const hookSpy = vi.hoisted(() => vi.fn())

vi.mock('@/hooks/usePendingSkillCandidates', () => ({
  usePendingSkillCandidates: () => hookSpy(),
}))

vi.mock('@/lib/tauri-api', () => ({
  approveSkillCandidate: vi.fn(),
  rejectSkillCandidate: vi.fn(),
}))

vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}))

vi.mock('@/lib/errorToast', () => ({
  toastError: vi.fn(),
}))

function makeCandidate(o: Partial<SkillCandidate> & { id: string }): SkillCandidate {
  return {
    detected_at: '2026-09-20T10:00:00Z',
    occurrence_count: 3,
    example_session_ids: ['s1', 's2'],
    proposed_name: `Candidate ${o.id}`,
    proposed_trigger: 'when the user commits',
    procedure: ['step one', 'step two'],
    source_tool_calls: [],
    ...o,
  }
}

function renderQueue(focusCandidateId: string | null = null) {
  return render(
    <MemoryRouter>
      <SkillCandidateReviewQueue focusCandidateId={focusCandidateId} />
    </MemoryRouter>,
  )
}

beforeEach(() => {
  hookSpy.mockReset()
  vi.mocked(api.approveSkillCandidate).mockReset()
  vi.mocked(api.rejectSkillCandidate).mockReset()
})

describe('SkillCandidateReviewQueue (Extensions → Pending)', () => {
  it('renders one card per pending candidate with trigger and procedure', () => {
    hookSpy.mockReturnValue({
      candidates: [
        makeCandidate({ id: 'cand-1' }),
        makeCandidate({ id: 'cand-2', proposed_name: 'Deploy helper' }),
      ],
      loading: false,
      refetch: vi.fn(),
    })
    renderQueue()
    const list = screen.getByRole('list', { name: 'Skill candidates' })
    expect(within(list).getByText('Candidate cand-1')).toBeInTheDocument()
    expect(within(list).getByText('Deploy helper')).toBeInTheDocument()
    expect(within(list).getAllByText('when the user commits')).toHaveLength(2)
    expect(within(list).getAllByText('step one')).toHaveLength(2)
  })

  it('shows the empty state when nothing is pending', () => {
    hookSpy.mockReturnValue({ candidates: [], loading: false, refetch: vi.fn() })
    renderQueue()
    expect(screen.getByText('Nothing waiting for review')).toBeInTheDocument()
    expect(screen.queryByRole('list', { name: 'Skill candidates' })).not.toBeInTheDocument()
  })

  it('shows skeletons while loading', () => {
    hookSpy.mockReturnValue({ candidates: [], loading: true, refetch: vi.fn() })
    const { container } = renderQueue()
    expect(container.querySelector('.animate-pulse')).not.toBeNull()
    expect(screen.queryByText('Nothing waiting for review')).not.toBeInTheDocument()
  })

  it('approve opens the review modal and calls approve_skill_candidate with edits', async () => {
    const refetch = vi.fn()
    hookSpy.mockReturnValue({
      candidates: [makeCandidate({ id: 'cand-1' })],
      loading: false,
      refetch,
    })
    vi.mocked(api.approveSkillCandidate).mockResolvedValue({
      id: 'cand-1', name: 'Candidate cand-1', description: '', trigger: '',
      procedure: [], created_at: '', originating_sessions: [],
    })
    renderQueue()

    // Card button opens the rich review modal (name/trigger editable).
    fireEvent.click(screen.getByRole('button', { name: 'Save "Candidate cand-1" as a skill' }))
    const dialog = await screen.findByRole('dialog', { name: 'Save as skill?' })
    const nameInput = within(dialog).getByLabelText('Name') as HTMLInputElement
    expect(nameInput.value).toBe('Candidate cand-1')

    // Confirm inside the modal → candidate approve command (the same path
    // the backend uses to resolve the matching inbox entry).
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save skill' }))
    await waitFor(() => expect(api.approveSkillCandidate).toHaveBeenCalledWith(
      'cand-1',
      { name: 'Candidate cand-1', trigger: 'when the user commits' },
    ))
    await waitFor(() => expect(refetch).toHaveBeenCalled())
  })

  it('reject calls reject_skill_candidate and refreshes the queue', async () => {
    const refetch = vi.fn()
    hookSpy.mockReturnValue({
      candidates: [makeCandidate({ id: 'cand-9', proposed_name: 'Deploy helper' })],
      loading: false,
      refetch,
    })
    vi.mocked(api.rejectSkillCandidate).mockResolvedValue(undefined)
    renderQueue()

    fireEvent.click(screen.getByRole('button', { name: 'Dismiss "Deploy helper"' }))
    await waitFor(() => expect(api.rejectSkillCandidate).toHaveBeenCalledWith('cand-9'))
    await waitFor(() => expect(refetch).toHaveBeenCalled())
  })

  it('marks the candidate handed over by the inbox card for one-shot focus', () => {
    hookSpy.mockReturnValue({
      candidates: [makeCandidate({ id: 'cand-1' }), makeCandidate({ id: 'cand-2' })],
      loading: false,
      refetch: vi.fn(),
    })
    renderQueue('cand-2')
    const list = screen.getByRole('list', { name: 'Skill candidates' })
    const focused = list.querySelector('[data-focus-candidate="true"]')
    expect(focused).not.toBeNull()
    expect(focused!.textContent).toContain('Candidate cand-2')
  })
})
