import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor, within } from '@testing-library/react'
import { AppProvider } from '@/context/AppContext'
import { MemoryRouter, useLocation } from 'react-router-dom'
import AdvancedSettings from '@/components/settings/AdvancedSettings'
import * as api from '@/lib/tauri-api'
import { setRemoteImagesAllowed, isRemoteImagesAllowed } from '@/lib/remoteImages'

// B2: the「open log directory」entry resolves $HOME/.shannon through the
// core path API (granted via capabilities) and opens it with the existing
// reveal command. Mock the path module so the test controls the result.
vi.mock('@tauri-apps/api/path', () => ({
  homeDir: vi.fn().mockResolvedValue('/home/tester'),
  join: vi.fn().mockResolvedValue('/home/tester/.shannon'),
}))
vi.mock('@tauri-apps/api/app', () => ({
  getVersion: vi.fn().mockResolvedValue('0.11.0'),
}))

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

  it('renders the open log directory entry with the real version badge', async () => {
    render(wrap(<AdvancedSettings />))
    expect(screen.getByText('Open log directory')).toBeInTheDocument()
    // B2: the version comes from getVersion() — the old fake logs modal
    // hardcoded "v0.1.0".
    await waitFor(() => expect(screen.getByText('v0.11.0')).toBeInTheDocument())
  })

  it('opens the Shannon log directory via openWithDefaultApp', async () => {
    const spy = vi.mocked(api.openWithDefaultApp).mockClear()
    render(wrap(<AdvancedSettings />))
    fireEvent.click(screen.getByText('Open log directory'))
    await waitFor(() => expect(spy).toHaveBeenCalledWith('/home/tester/.shannon'))
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

  // Settings R3 (T1): the「版本与更新」card moved to Settings → 关于 — this
  // page keeps only a cross-link into the About section. The update-check
  // behaviour itself is covered by AboutSettings.test.tsx.
  it('links to the About section where the update check now lives', () => {
    render(wrap(<AdvancedSettings />))
    const link = screen.getByTestId('updates-moved-link')
    expect(within(link).getByText('Updates moved to About')).toBeInTheDocument()
    expect(link.querySelector('a')).toHaveAttribute('href', '/settings/about')
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

  // 卡A GC — session storage management card, next to the dream block.
  // The description carries the informed-consent copy the review required
  // (archived-only, last-activity clocked, never-auto-delete by default).
  it('renders the session storage management card with the required disclosure copy', () => {
    render(wrap(<AdvancedSettings />))
    const card = screen.getByTestId('session-gc-card')
    expect(within(card).getByText('Session storage management')).toBeInTheDocument()
    const desc = within(card).getByText(/Automatically free storage by cleaning up archived sessions/)
    const copy = desc.textContent ?? ''
    expect(copy).toMatch(/Only archived sessions are ever cleaned/)
    expect(copy).toMatch(/last activity/)
    expect(copy).toMatch(/nothing is auto-deleted by default/)
  })

  it('renders the GC switch default-off and the retention select defaulting to Never', () => {
    render(wrap(<AdvancedSettings />))
    const card = screen.getByTestId('session-gc-card')
    expect(within(card).getByRole('switch')).toHaveAttribute('aria-checked', 'false')
    const select = within(card).getByRole('combobox', { name: 'Retention window' }) as HTMLSelectElement
    expect(select.value).toBe('0')
    expect(select.selectedOptions[0].textContent).toBe('Never')
  })

  it('persists session_gc_enabled through configure when toggled', async () => {
    render(wrap(<AdvancedSettings />))
    const card = screen.getByTestId('session-gc-card')
    fireEvent.click(within(card).getByRole('switch'))
    await waitFor(() => {
      expect(api.configure).toHaveBeenCalledWith({ key: 'session_gc_enabled', value: 'true' })
    })
  })

  it('persists retention 30 days through configure as session_retention_days=30', async () => {
    render(wrap(<AdvancedSettings />))
    const card = screen.getByTestId('session-gc-card')
    fireEvent.change(within(card).getByRole('combobox', { name: 'Retention window' }), { target: { value: '30' } })
    await waitFor(() => {
      expect(api.configure).toHaveBeenCalledWith({ key: 'session_retention_days', value: '30' })
    })
  })

  it('persists the 永不 gear as session_retention_days=0 (0 means never)', async () => {
    vi.mocked(api.getConfig).mockResolvedValueOnce({
      provider: 'anthropic',
      model: 'claude-sonnet-4-6',
      session_retention_days: 30,
    } as any)
    render(wrap(<AdvancedSettings />))
    const card = screen.getByTestId('session-gc-card')
    // Fixture: a persisted 30-day window shows as the selected gear.
    const select = await within(card).findByRole('combobox', { name: 'Retention window' }) as HTMLSelectElement
    await waitFor(() => expect(select.value).toBe('30'))
    fireEvent.change(select, { target: { value: '0' } })
    await waitFor(() => {
      expect(api.configure).toHaveBeenCalledWith({ key: 'session_retention_days', value: '0' })
    })
  })

  // P2-4 (R9-④) — remote images card. Persistence is frontend-local (the
  // lib/remoteImages localStorage store, like the theme/density keys): the
  // switch drives the store directly and must NOT touch api.configure.
  it('renders the remote-images card with the switch defaulting to off and the motivation copy', () => {
    setRemoteImagesAllowed(false)
    render(wrap(<AdvancedSettings />))
    const card = screen.getByTestId('remote-images-card')
    expect(within(card).getByRole('switch')).toHaveAttribute('aria-checked', 'false')
    expect(within(card).getByText(/exfiltrate session content via their URLs/i)).toBeInTheDocument()
  })

  it('persists the remote-images switch to the frontend-local store, not configure', () => {
    setRemoteImagesAllowed(false)
    // The configure spy accumulates calls from the sibling tests above —
    // clear it so this test proves exactly its own switch's write path.
    vi.mocked(api.configure).mockClear()
    render(wrap(<AdvancedSettings />))
    const card = screen.getByTestId('remote-images-card')
    fireEvent.click(within(card).getByRole('switch'))
    expect(within(card).getByRole('switch')).toHaveAttribute('aria-checked', 'true')
    expect(localStorage.getItem('shannon.chat.allowRemoteImages')).toBe('1')
    expect(isRemoteImagesAllowed()).toBe(true)
    expect(api.configure).not.toHaveBeenCalled()
    setRemoteImagesAllowed(false)
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
