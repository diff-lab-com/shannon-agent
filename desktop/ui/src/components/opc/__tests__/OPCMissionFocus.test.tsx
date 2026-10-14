import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import OPCMissionFocus from '@/components/opc/OPCMissionFocus'
import * as api from '@/lib/tauri-api'
import type { MissionConfig, TaskItem } from '@/types'

vi.mock('@/lib/tauri-api', () => ({
  configure: vi.fn().mockResolvedValue(undefined),
  missionProgress: vi.fn(),
}))

vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}))

const missionProgress = vi.mocked(api.missionProgress)

const FUTURE_TS = new Date('2027-03-01').getTime()
const PAST_TS = new Date('2020-01-01').getTime()

const mission: MissionConfig = {
  name: 'Launch Shannon v2',
  budget_usd: 120,
  deadline_ts: FUTURE_TS,
  task_ids: ['t1'],
}

const boardTasks: TaskItem[] = [
  { id: 't1', title: 'A', status: 'in_progress' },
  { id: 't2', title: 'B', status: 'in_progress' },
  { id: 't3', title: 'C', status: 'completed' },
  { id: 't4', title: 'D', status: 'pending' },
]

function renderFocus(
  config: { provider?: string; strategic_focus?: string } | null = null,
  opts: { mission?: MissionConfig | null; onManageMission?: () => void; tasks?: TaskItem[] } = {},
) {
  return render(
    <MemoryRouter>
      <OPCMissionFocus
        config={config}
        tasks={opts.tasks ?? []}
        mission={opts.mission ?? null}
        onManageMission={opts.onManageMission}
      />
    </MemoryRouter>
  )
}

beforeEach(() => {
  missionProgress.mockReset()
  missionProgress.mockResolvedValue({ name: 'Launch Shannon v2', tasks: [] })
})

describe('OPCMissionFocus', () => {
  it('renders default heading without config', () => {
    renderFocus(null)
    expect(screen.getByText(/Autonomous task execution through multi-agent/)).toBeInTheDocument()
  })

  it('renders provider-based heading with config', () => {
    renderFocus({ provider: 'anthropic' })
    expect(screen.getByText(/Anthropic Agent Orchestration/)).toBeInTheDocument()
  })

  it('shows today\'s mission label', () => {
    renderFocus()
    expect(screen.getByText("Today's Mission")).toBeInTheDocument()
  })

  it('shows Edit button', () => {
    renderFocus()
    expect(screen.getByText('Edit')).toBeInTheDocument()
  })

  it('toggles to textarea on Edit click', () => {
    renderFocus()
    fireEvent.click(screen.getByText('Edit'))
    expect(screen.getByText('Save Mission')).toBeInTheDocument()
    expect(screen.getByLabelText("Edit today's mission")).toBeInTheDocument()
  })

  it('reverts to display mode on Cancel', () => {
    renderFocus()
    fireEvent.click(screen.getByText('Edit'))
    expect(screen.getByText('Cancel')).toBeInTheDocument()
    fireEvent.click(screen.getByText('Cancel'))
    expect(screen.getByText('Edit')).toBeInTheDocument()
  })

  it('shows custom strategic_focus when set', () => {
    renderFocus({ strategic_focus: 'Ship the MVP by Q3.' })
    expect(screen.getByText('Ship the MVP by Q3.')).toBeInTheDocument()
  })

  it('has aria-expanded reflecting edit state', () => {
    renderFocus()
    const btn = screen.getByRole('button', { name: 'Edit' })
    expect(btn).toHaveAttribute('aria-expanded', 'false')
    fireEvent.click(btn)
    expect(btn).toHaveAttribute('aria-expanded', 'true')
  })
})

// ─── G4 (R2 audit §4-G4): mission mode — single hero absorbing the
// MissionCard's budget/deadline chips, editor moved to the manage dialog ───

describe('OPCMissionFocus — mission mode (G4 hero merge)', () => {
  it('renders the mission name as the headline instead of the focus text', () => {
    renderFocus({ strategic_focus: 'Ship the MVP by Q3.' }, { mission })
    expect(screen.getByText('Launch Shannon v2')).toBeInTheDocument()
    expect(screen.queryByText('Ship the MVP by Q3.')).not.toBeInTheDocument()
    // The strategic-focus inline editor has no business in mission mode.
    expect(screen.queryByText('Edit')).not.toBeInTheDocument()
  })

  it('absorbs the budget and deadline chips (design 09 三要素)', () => {
    renderFocus(null, { mission })
    expect(screen.getByTestId('mission-hero-budget')).toHaveTextContent('Budget $120.00')
    // Compact hero chip: month + day only (design 09 shows 「10-09」).
    expect(screen.getByTestId('mission-hero-deadline').textContent).toMatch(/Due/)
    expect(screen.getByTestId('mission-hero-deadline').textContent).toMatch(/Mar/)
    // Future deadline — no overdue marker.
    expect(screen.queryByTestId('mission-hero-overdue')).not.toBeInTheDocument()
  })

  it('shows the measured spend against the budget when the ledger has one', async () => {
    missionProgress.mockResolvedValue({
      name: 'Launch Shannon v2',
      budget_usd: 120,
      budget_used_usd: 30,
      tasks: [],
    })
    renderFocus(null, { mission })
    await waitFor(() =>
      expect(screen.getByTestId('mission-hero-budget')).toHaveTextContent('$30.00'),
    )
    expect(screen.getByTestId('mission-hero-budget')).toHaveTextContent('$120.00')
  })

  it('keeps the progress strip aggregate in mission mode', () => {
    renderFocus(null, { mission, tasks: boardTasks })
    const strip = screen.getByTestId('opc-mission-progress')
    expect(strip).toHaveTextContent('In progress 2')
    expect(strip).toHaveTextContent('Completed 1')
    expect(strip).toHaveTextContent('4 total')
  })

  it('hides the budget chip and keeps the headline honest when fields are absent', () => {
    renderFocus(null, { mission: { name: 'Solo run', task_ids: [] } })
    expect(screen.getByText('Solo run')).toBeInTheDocument()
    expect(screen.queryByTestId('mission-hero-budget')).not.toBeInTheDocument()
    expect(screen.queryByTestId('mission-hero-deadline')).not.toBeInTheDocument()
  })

  it('marks a past deadline as overdue', () => {
    renderFocus(null, { mission: { ...mission, deadline_ts: PAST_TS } })
    expect(screen.getByTestId('mission-hero-overdue')).toHaveTextContent('Overdue')
  })

  it('opens the manage-mission dialog through the page callback', () => {
    const onManageMission = vi.fn()
    renderFocus(null, { mission, onManageMission })
    fireEvent.click(screen.getByTestId('mission-manage-open'))
    expect(onManageMission).toHaveBeenCalledTimes(1)
  })

  it('hides the manage affordance when no handler is wired', () => {
    renderFocus(null, { mission })
    expect(screen.queryByTestId('mission-manage-open')).not.toBeInTheDocument()
  })
})
