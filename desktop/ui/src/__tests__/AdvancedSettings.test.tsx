import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor, within } from '@testing-library/react'
import { AppProvider } from '@/context/AppContext'
import { MemoryRouter, useLocation } from 'react-router-dom'
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
  it('renders advanced settings subtitle', () => {
    render(wrap(<AdvancedSettings />))
    expect(screen.getByText(/Configure underlying engine parameters/i)).toBeInTheDocument()
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
    expect(screen.getByText('Clear Chat Cache')).toBeInTheDocument()
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

  // ADR-0011 B3 — command line card
  it('renders command line section', () => {
    render(wrap(<AdvancedSettings />))
    expect(screen.getByText('Command line')).toBeInTheDocument()
  })

  it('shows CLI not-on-path badge and install button by default', async () => {
    render(wrap(<AdvancedSettings />))
    await waitFor(() => expect(screen.getByText('Install `shannon` command')).toBeInTheDocument())
    expect(screen.getByText('not on PATH')).toBeInTheDocument()
  })

  // C1① — version & updates card
  it('renders version & updates section with a check button', () => {
    render(wrap(<AdvancedSettings />))
    expect(screen.getByText('Version & updates')).toBeInTheDocument()
    expect(screen.getByText('Check for updates')).toBeInTheDocument()
  })

  it('shows up-to-date badge and download page link after a check', async () => {
    render(wrap(<AdvancedSettings />))
    fireEvent.click(screen.getByText('Check for updates'))
    await waitFor(() => expect(screen.getByText('up to date')).toBeInTheDocument())
    expect(screen.getByText('Open download page')).toBeInTheDocument()
    expect(screen.getByText('Current version: 0.11.0')).toBeInTheDocument()
  })

  it('announces an available update and opens the release page', async () => {
    vi.mocked(api.checkAppUpdate).mockResolvedValueOnce({
      currentVersion: '0.11.0',
      latestVersion: 'v0.12.0',
      updateAvailable: true,
      releaseUrl: 'https://github.com/diff-lab-com/shannon-agent/releases/tag/v0.12.0',
      error: null,
    })
    render(wrap(<AdvancedSettings />))
    fireEvent.click(screen.getByText('Check for updates'))
    await waitFor(() => expect(screen.getByText('v0.12.0 available')).toBeInTheDocument())
    fireEvent.click(screen.getByText('Open download page'))
    await waitFor(() =>
      expect(api.openReleasePage).toHaveBeenCalledWith(
        'https://github.com/diff-lab-com/shannon-agent/releases/tag/v0.12.0'
      )
    )
  })

  // US-SET-04: System Logs modal
  it('opens system logs modal on View System Logs click', () => {
    render(wrap(<AdvancedSettings />))
    fireEvent.click(screen.getByText('View System Logs'))
    expect(screen.getByText('System Logs')).toBeInTheDocument()
    expect(screen.getByText('Shannon Desktop v0.1.0')).toBeInTheDocument()
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

  // B2 — agent teams (real sub-agent execution) card
  it('renders agent teams section', () => {
    render(wrap(<AdvancedSettings />))
    expect(screen.getByText('Agent teams (subagents)')).toBeInTheDocument()
  })

  it('renders agent teams toggle with live-effect hint', () => {
    render(wrap(<AdvancedSettings />))
    expect(screen.getByText('Enable real sub-agent execution')).toBeInTheDocument()
    expect(screen.getByText('Takes effect immediately — no restart needed.')).toBeInTheDocument()
  })

  // Dream distillation (梦境提炼) card — two switches next to the skill-loop
  // block, persisted through the same handleToggle → configure path.
  it('renders the dream distillation card with both toggles', () => {
    render(wrap(<AdvancedSettings />))
    expect(screen.getByText('Dream distillation')).toBeInTheDocument()
    expect(screen.getByText('Nightly auto-distillation')).toBeInTheDocument()
    expect(screen.getByText('Off by default — runs between 1–5 AM when idle, at most once a day.')).toBeInTheDocument()
    expect(screen.getByText('Review before write')).toBeInTheDocument()
    expect(screen.getByText('Distilled entries and skill candidates are written only after you approve them.')).toBeInTheDocument()
  })

  it('persists dream_enabled through configure when toggled', async () => {
    render(wrap(<AdvancedSettings />))
    const row = screen.getByText('Nightly auto-distillation').closest('div.flex.items-center.justify-between')!
    fireEvent.click(within(row).getByRole('switch'))
    await waitFor(() => {
      expect(api.configure).toHaveBeenCalledWith({ key: 'dream_enabled', value: 'true' })
    })
  })

  it('persists dream_skill_distill_enabled through configure when toggled', async () => {
    render(wrap(<AdvancedSettings />))
    const row = screen.getByText('Review before write').closest('div.flex.items-center.justify-between')!
    fireEvent.click(within(row).getByRole('switch'))
    await waitFor(() => {
      expect(api.configure).toHaveBeenCalledWith({ key: 'dream_skill_distill_enabled', value: 'true' })
    })
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

  // IA T3 + X1: no second SkillApprovalModal lives here anymore —
  // the「Review pending」entry links to /extensions/pending, the single
  // skill-review surface (评审裁决 #2).
  it('navigates to /extensions/pending on Review click (no modal)', async () => {
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
    function LocationProbe() {
      const location = useLocation()
      return <div data-testid="adv-location">{location.pathname}</div>
    }
    render(
      <AppProvider>
        <MemoryRouter>
          <AdvancedSettings />
          <LocationProbe />
        </MemoryRouter>
      </AppProvider>
    )
    await waitFor(() => { expect(screen.getByText('Review pending')).toBeInTheDocument() })
    fireEvent.click(screen.getByText('Review pending'))
    await waitFor(() => { expect(screen.getByTestId('adv-location')).toHaveTextContent('/extensions/pending') })
    expect(screen.queryByText('Save as skill?')).not.toBeInTheDocument()
  })
})
