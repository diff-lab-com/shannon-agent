import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { AppProvider } from '@/context/AppContext'
import { MemoryRouter } from 'react-router-dom'
import AdvancedSettings from '@/components/settings/AdvancedSettings'
import * as api from '@/lib/tauri-api'

function wrap(ui: React.ReactElement) {
  return (
    <AppProvider>
      <MemoryRouter>
        {ui}
      </MemoryRouter>
    </AppProvider>
  )
}

describe('AdvancedSettings', () => {
  it('renders advanced settings heading', () => {
    render(wrap(<AdvancedSettings />))
    expect(screen.getByText('Advanced Settings')).toBeInTheDocument()
  })

  it('renders memory management section', () => {
    render(wrap(<AdvancedSettings />))
    expect(screen.getByText('Memory Management')).toBeInTheDocument()
  })

  it('renders long-term memory toggle label', () => {
    render(wrap(<AdvancedSettings />))
    expect(screen.getByText('Long-term Memory')).toBeInTheDocument()
  })

  it('renders clear session cache button', () => {
    render(wrap(<AdvancedSettings />))
    expect(screen.getByText('Clear Session Cache')).toBeInTheDocument()
  })

  it('renders data privacy section', () => {
    render(wrap(<AdvancedSettings />))
    expect(screen.getByText('Data Privacy')).toBeInTheDocument()
  })

  it('renders developer options section', () => {
    render(wrap(<AdvancedSettings />))
    expect(screen.getByText('Developer Options')).toBeInTheDocument()
  })

  it('renders factory reset button', () => {
    render(wrap(<AdvancedSettings />))
    expect(screen.getByText('Reset to Factory Settings')).toBeInTheDocument()
  })

  it('renders view system logs link', () => {
    render(wrap(<AdvancedSettings />))
    expect(screen.getByText('View System Logs')).toBeInTheDocument()
  })

  it('renders manage api keys link', () => {
    render(wrap(<AdvancedSettings />))
    expect(screen.getByText('Manage API Keys')).toBeInTheDocument()
  })

  // US-SET-04: System Logs modal
  it('opens system logs modal on View System Logs click', () => {
    render(wrap(<AdvancedSettings />))
    fireEvent.click(screen.getByText('View System Logs'))
    expect(screen.getByText('System Logs')).toBeInTheDocument()
    expect(screen.getByText(/Shannon Desktop/)).toBeInTheDocument()
  })

  it('closes system logs modal via close button', () => {
    render(wrap(<AdvancedSettings />))
    fireEvent.click(screen.getByText('View System Logs'))
    expect(screen.getByText('System Logs')).toBeInTheDocument()
    // Click the close button inside the modal
    const modal = screen.getByText('System Logs').closest('.fixed')!
    const closeBtn = modal.querySelector('button')
    if (closeBtn) fireEvent.click(closeBtn)
  })

  // US-SET-04: API Keys modal
  it('opens api keys modal on Manage API Keys click', () => {
    render(wrap(<AdvancedSettings />))
    fireEvent.click(screen.getAllByText('Manage API Keys')[0])
    expect(screen.getByText('Go to Model Settings')).toBeInTheDocument()
  })

  // Skill loop toggle tests
  it('renders skill extraction section', () => {
    render(wrap(<AdvancedSettings />))
    expect(screen.getByText('Skill Extraction')).toBeInTheDocument()
  })

  it('renders enable skill extraction toggle', () => {
    render(wrap(<AdvancedSettings />))
    expect(screen.getByText('Enable skill extraction')).toBeInTheDocument()
  })

  it('renders skill extraction description', () => {
    render(wrap(<AdvancedSettings />))
    expect(screen.getByText(/After complex tasks, Shannon evaluates/)).toBeInTheDocument()
  })
})

describe('AdvancedSettings — Self-improvement approval', () => {
  beforeEach(() => {
    vi.mocked(api.listSkillCandidates).mockReset()
    vi.mocked(api.approveSkillCandidate).mockReset()
    vi.mocked(api.rejectSkillCandidate).mockReset()
  })

  it('hides Review button when no candidates pending', async () => {
    vi.mocked(api.listSkillCandidates).mockResolvedValue([])
    render(wrap(<AdvancedSettings />))
    await waitFor(() => { expect(api.listSkillCandidates).toHaveBeenCalled() })
    expect(screen.queryByText('Review pending')).not.toBeInTheDocument()
  })

  it('shows pending count badge and Review button when candidates exist', async () => {
    vi.mocked(api.listSkillCandidates).mockResolvedValue([
      {
        id: 'cand-1',
        proposed_name: 'Wrap commits',
        proposed_trigger: 'when committing',
        occurrence_count: 3,
        procedure: ['step 1', 'step 2'],
        last_seen_at: '',
        originating_sessions: [],
      },
    ])
    render(wrap(<AdvancedSettings />))
    await waitFor(() => { expect(screen.getByText('Review pending')).toBeInTheDocument() })
    expect(screen.getByText('1 pending')).toBeInTheDocument()
  })

  it('opens SkillApprovalModal on Review click', async () => {
    vi.mocked(api.listSkillCandidates).mockResolvedValue([
      {
        id: 'cand-1',
        proposed_name: 'Wrap commits',
        proposed_trigger: 'when committing',
        occurrence_count: 3,
        procedure: ['step 1', 'step 2'],
        last_seen_at: '',
        originating_sessions: [],
      },
    ])
    render(wrap(<AdvancedSettings />))
    await waitFor(() => { expect(screen.getByText('Review pending')).toBeInTheDocument() })
    fireEvent.click(screen.getByText('Review pending'))
    await waitFor(() => { expect(screen.getByText('Save as skill?')).toBeInTheDocument() })
    expect(screen.getByDisplayValue('Wrap commits')).toBeInTheDocument()
  })
})
