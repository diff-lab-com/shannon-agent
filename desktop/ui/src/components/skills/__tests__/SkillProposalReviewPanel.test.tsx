import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor, act } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import SkillProposalReviewPanel from '../SkillProposalReviewPanel'
import * as api from '@/lib/tauri-api'

vi.mock('@/lib/tauri-api', () => ({
  skillLoop: {
    listProposals: vi.fn(),
    approve: vi.fn(),
    reject: vi.fn(),
  },
}))

vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}))

// X1 fix: the panel refreshes its draft list on `skill-proposal-available`;
// capture the handler so tests can fire the backend event.
const eventSpy = vi.hoisted(() => vi.fn())

vi.mock('@/hooks/useTauriEventValidated', () => ({
  useTauriEventValidated: (...args: unknown[]) => eventSpy(...args),
}))

let proposalEventHandler: ((event: { payload: { pending_count: number } }) => void) | null = null

function emitProposalEvent(pendingCount: number) {
  act(() => {
    proposalEventHandler?.({ payload: { pending_count: pendingCount } })
  })
}

const mockProposal = {
  id: 'test-id-1',
  name: 'Test Skill',
  slug: 'test-skill',
  description: 'A test skill proposal',
  trigger_patterns: ['when user asks to test'],
  example_workflow: '1. Step one\n2. Step two',
  source_task_id: 'task-123',
  created_at: '2025-06-23T10:00:00Z',
  status: 'Pending' as const,
}

describe('SkillProposalReviewPanel', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    eventSpy.mockReset()
    proposalEventHandler = null
    eventSpy.mockImplementation((_event: string, handler: (e: { payload: { pending_count: number } }) => void) => {
      proposalEventHandler = handler
    })
  })

  it('renders nothing when closed', () => {
    render(
      <I18nProvider>
        <SkillProposalReviewPanel open={false} onClose={vi.fn()} />
      </I18nProvider>
    )

    expect(screen.queryByText('Skill Proposals')).not.toBeInTheDocument()
  })

  it('renders title when open', () => {
    vi.mocked(api.skillLoop.listProposals).mockResolvedValue([])

    render(
      <I18nProvider>
        <SkillProposalReviewPanel open={true} onClose={vi.fn()} />
      </I18nProvider>
    )

    expect(screen.getByText('Skill Proposals')).toBeInTheDocument()
  })

  it('displays proposal cards', async () => {
    vi.mocked(api.skillLoop.listProposals).mockResolvedValue([mockProposal])

    render(
      <I18nProvider>
        <SkillProposalReviewPanel open={true} onClose={vi.fn()} />
      </I18nProvider>
    )

    await waitFor(() => {
      expect(screen.getByText('Test Skill')).toBeInTheDocument()
      expect(screen.getByText('A test skill proposal')).toBeInTheDocument()
      expect(screen.getByText('when user asks to test')).toBeInTheDocument()
    })
  })

  it('shows empty state when no proposals', async () => {
    vi.mocked(api.skillLoop.listProposals).mockResolvedValue([])

    render(
      <I18nProvider>
        <SkillProposalReviewPanel open={true} onClose={vi.fn()} />
      </I18nProvider>
    )

    await waitFor(() => {
      expect(screen.getByText('No suggestions yet')).toBeInTheDocument()
    })
  })

  it('calls approve API and removes card on Approve button click', async () => {
    vi.mocked(api.skillLoop.listProposals).mockResolvedValue([mockProposal])
    vi.mocked(api.skillLoop.approve).mockResolvedValue('/path/to/skill.toml')

    const onClose = vi.fn()
    render(
      <I18nProvider>
        <SkillProposalReviewPanel open={true} onClose={onClose} />
      </I18nProvider>
    )

    await waitFor(() => {
      expect(screen.getByText('Test Skill')).toBeInTheDocument()
    })

    const approveButton = screen.getByText('Approve')
    fireEvent.click(approveButton)

    await waitFor(() => {
      expect(api.skillLoop.approve).toHaveBeenCalledWith('test-id-1')
      expect(screen.queryByText('Test Skill')).not.toBeInTheDocument()
    })
  })

  it('calls reject API and removes card on Reject button click', async () => {
    vi.mocked(api.skillLoop.listProposals).mockResolvedValue([mockProposal])
    vi.mocked(api.skillLoop.reject).mockResolvedValue()

    const onClose = vi.fn()
    render(
      <I18nProvider>
        <SkillProposalReviewPanel open={true} onClose={onClose} />
      </I18nProvider>
    )

    await waitFor(() => {
      expect(screen.getByText('Test Skill')).toBeInTheDocument()
    })

    const rejectButton = screen.getByText('Reject')
    fireEvent.click(rejectButton)

    await waitFor(() => {
      expect(api.skillLoop.reject).toHaveBeenCalledWith('test-id-1')
      expect(screen.queryByText('Test Skill')).not.toBeInTheDocument()
    })
  })

  it('closes on Escape key', async () => {
    vi.mocked(api.skillLoop.listProposals).mockResolvedValue([])

    const onClose = vi.fn()
    render(
      <I18nProvider>
        <SkillProposalReviewPanel open={true} onClose={onClose} />
      </I18nProvider>
    )

    fireEvent.keyDown(document, { key: 'Escape', code: 'Escape' })

    await waitFor(() => {
      expect(onClose).toHaveBeenCalledTimes(1)
    })
  })

  it('shows navigation for multiple proposals', async () => {
    const mockProposal2 = { ...mockProposal, id: 'test-id-2', name: 'Test Skill 2' }
    vi.mocked(api.skillLoop.listProposals).mockResolvedValue([mockProposal, mockProposal2])

    render(
      <I18nProvider>
        <SkillProposalReviewPanel open={true} onClose={vi.fn()} />
      </I18nProvider>
    )

    await waitFor(() => {
      expect(screen.getByText('Test Skill')).toBeInTheDocument()
      expect(screen.getByText(/1 \/ 2/)).toBeInTheDocument()
      expect(screen.getByText('Previous')).toBeInTheDocument()
      expect(screen.getByText('Next')).toBeInTheDocument()
    })
  })
})

// IA X1: the panel embeds into the Extensions → Pending queue in inline
// mode — same review UI, no modal chrome, no auto-close.
describe('SkillProposalReviewPanel (inline variant)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('renders nothing when there are no proposals', async () => {
    vi.mocked(api.skillLoop.listProposals).mockResolvedValue([])

    const { container } = render(
      <I18nProvider>
        <SkillProposalReviewPanel open={true} onClose={vi.fn()} variant="inline" />
      </I18nProvider>
    )

    await waitFor(() => {
      expect(container.querySelector('[data-testid="skill-proposal-review-inline"]')).toBeNull()
    })
  })

  it('renders the proposal without a modal and approves without closing', async () => {
    vi.mocked(api.skillLoop.listProposals).mockResolvedValue([mockProposal])
    vi.mocked(api.skillLoop.approve).mockResolvedValue('/path/to/skill.toml')

    const onClose = vi.fn()
    const { container } = render(
      <I18nProvider>
        <SkillProposalReviewPanel open={true} onClose={onClose} variant="inline" />
      </I18nProvider>
    )

    await waitFor(() => {
      expect(container.querySelector('[data-testid="skill-proposal-review-inline"]')).not.toBeNull()
    })
    expect(screen.getByText('Test Skill')).toBeInTheDocument()
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()

    fireEvent.click(screen.getByText('Approve'))
    await waitFor(() => expect(api.skillLoop.approve).toHaveBeenCalledWith('test-id-1'))
    // Inline mode stays mounted — after the last proposal is gone the
    // block simply unmounts; onClose is never called.
    expect(onClose).not.toHaveBeenCalled()
    await waitFor(() => {
      expect(container.querySelector('[data-testid="skill-proposal-review-inline"]')).toBeNull()
    })
  })

  // X1 fix: a draft created mid-session must show up while the user sits on
  // /extensions/pending — the backend event triggers an in-place refetch.
  it('refreshes the draft list when skill-proposal-available fires', async () => {
    vi.mocked(api.skillLoop.listProposals).mockResolvedValue([mockProposal])
    const { container } = render(
      <I18nProvider>
        <SkillProposalReviewPanel open={true} onClose={vi.fn()} variant="inline" />
      </I18nProvider>
    )
    await waitFor(() => expect(screen.getByText('Test Skill')).toBeInTheDocument())
    expect(api.skillLoop.listProposals).toHaveBeenCalledTimes(1)

    const arrival = { ...mockProposal, id: 'test-id-2', name: 'Test Skill 2' }
    vi.mocked(api.skillLoop.listProposals).mockResolvedValue([mockProposal, arrival])
    emitProposalEvent(2)

    // The panel re-fetched and the queue now reports two drafts (the cycler
    // shows one at a time — the navigation counter reflects the refresh).
    await waitFor(() => expect(api.skillLoop.listProposals).toHaveBeenCalledTimes(2))
    await waitFor(() => expect(screen.getByText(/1 \/ 2/)).toBeInTheDocument())
    await waitFor(() => expect(container.querySelector('[data-testid="skill-proposal-review-inline"]')).not.toBeNull())
  })

  it('does not refetch on the event while closed', async () => {
    vi.mocked(api.skillLoop.listProposals).mockResolvedValue([])
    render(
      <I18nProvider>
        <SkillProposalReviewPanel open={false} onClose={vi.fn()} />
      </I18nProvider>
    )
    await waitFor(() => expect(proposalEventHandler).not.toBeNull())
    expect(api.skillLoop.listProposals).not.toHaveBeenCalled()
    emitProposalEvent(4)
    expect(api.skillLoop.listProposals).not.toHaveBeenCalled()
  })
})
