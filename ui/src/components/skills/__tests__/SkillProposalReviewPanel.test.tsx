import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
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
