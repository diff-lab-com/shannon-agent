import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import { AgentAuthoredBadge } from '@/components/self-improve/SkillBadge'
import { SkillApprovalModal } from '@/components/self-improve/SkillApprovalModal'
import * as api from '@/lib/tauri-api'
import type { SkillCandidate } from '@/lib/tauri-api'

function renderWithI18n(ui: React.ReactNode) {
  return render(<I18nProvider>{ui}</I18nProvider>)
}

const sampleCandidate: SkillCandidate = {
  id: 'cand-1',
  detected_at: '2026-06-26T13:00:00Z',
  occurrence_count: 3,
  example_session_ids: ['s1', 's2', 's3'],
  proposed_name: 'Refactor React component',
  proposed_trigger: 'when the user asks to refactor a React component',
  procedure: [
    'Read the target file',
    'Identify props and hooks',
    'Extract subcomponents',
    'Update imports',
  ],
  source_tool_calls: [
    { tool: 'read_file', args_summary: { path: 'src/Foo.tsx' } },
    { tool: 'edit_file', args_summary: { path: 'src/Foo.tsx' } },
  ],
}

describe('AgentAuthoredBadge', () => {
  it('renders the agent-authored label', () => {
    renderWithI18n(<AgentAuthoredBadge />)
    expect(screen.getByText('Agent-authored')).toBeInTheDocument()
  })

  it('includes the auto_fix icon', () => {
    const { container } = renderWithI18n(<AgentAuthoredBadge />)
    expect(container.querySelector('.material-symbols-outlined')?.textContent).toContain('auto_fix')
  })
})

describe('SkillApprovalModal', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(api.approveSkillCandidate).mockReset()
    vi.mocked(api.rejectSkillCandidate).mockReset()
  })

  it('renders nothing when candidate is null', () => {
    const { container } = renderWithI18n(
      <SkillApprovalModal open={false} candidate={null} onClose={() => {}} />,
    )
    expect(container).toBeEmptyDOMElement()
  })

  it('shows the candidate name + trigger when open', () => {
    renderWithI18n(
      <SkillApprovalModal open candidate={sampleCandidate} onClose={() => {}} />,
    )
    expect(screen.getByDisplayValue('Refactor React component')).toBeInTheDocument()
    expect(screen.getByDisplayValue(/refactor a React component/)).toBeInTheDocument()
  })

  it('shows occurrence count in description', () => {
    renderWithI18n(
      <SkillApprovalModal open candidate={sampleCandidate} onClose={() => {}} />,
    )
    expect(screen.getByText(/Detected 3 similar workflows/)).toBeInTheDocument()
  })

  it('lists each procedure step', () => {
    renderWithI18n(
      <SkillApprovalModal open candidate={sampleCandidate} onClose={() => {}} />,
    )
    expect(screen.getByText('Read the target file')).toBeInTheDocument()
    expect(screen.getByText('Extract subcomponents')).toBeInTheDocument()
  })

  it('Approve button calls approveSkillCandidate with current name + trigger', async () => {
    vi.mocked(api.approveSkillCandidate).mockResolvedValue({
      id: 'skill-1',
      name: 'Refactor React component',
      description: '',
      trigger: 'when the user asks to refactor a React component',
      procedure: [],
      created_at: '',
      originating_sessions: [],
    })
    renderWithI18n(
      <SkillApprovalModal open candidate={sampleCandidate} onClose={() => {}} />,
    )
    fireEvent.click(screen.getByText('Save skill'))
    await waitFor(() => {
      expect(api.approveSkillCandidate).toHaveBeenCalledWith('cand-1', {
        name: 'Refactor React component',
        trigger: 'when the user asks to refactor a React component',
      })
    })
  })

  it('Reject button calls rejectSkillCandidate', async () => {
    vi.mocked(api.rejectSkillCandidate).mockResolvedValue(undefined)
    renderWithI18n(
      <SkillApprovalModal open candidate={sampleCandidate} onClose={() => {}} />,
    )
    fireEvent.click(screen.getByText('Reject'))
    await waitFor(() => {
      expect(api.rejectSkillCandidate).toHaveBeenCalledWith('cand-1')
    })
  })

  it('Approve button disabled when name is empty', () => {
    renderWithI18n(
      <SkillApprovalModal open candidate={sampleCandidate} onClose={() => {}} />,
    )
    const nameInput = screen.getByDisplayValue('Refactor React component')
    fireEvent.change(nameInput, { target: { value: '' } })
    expect(screen.getByText('Save skill')).toBeDisabled()
  })

  it('handles approve error gracefully', async () => {
    vi.mocked(api.approveSkillCandidate).mockRejectedValue(new Error('Disk full'))
    renderWithI18n(
      <SkillApprovalModal open candidate={sampleCandidate} onClose={() => {}} />,
    )
    fireEvent.click(screen.getByText('Save skill'))
    await waitFor(() => {
      expect(api.approveSkillCandidate).toHaveBeenCalledTimes(1)
    })
  })
})

describe('Self-improve API stubs', () => {
  it('listSkillCandidates invokes list_skill_candidates', async () => {
    vi.mocked(api.listSkillCandidates).mockResolvedValue([])
    const result = await api.listSkillCandidates()
    expect(result).toEqual([])
  })

  it('listAgentAuthoredSkills invokes list_agent_authored_skills', async () => {
    vi.mocked(api.listAgentAuthoredSkills).mockResolvedValue([])
    const result = await api.listAgentAuthoredSkills()
    expect(result).toEqual([])
  })
})
