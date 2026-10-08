import { describe, it, expect, vi } from 'vitest'
import { render, screen, waitFor, fireEvent } from '@testing-library/react'
import { AppProvider } from '@/context/AppContext'
import { MemoryRouter } from 'react-router-dom'
import { I18nProvider } from '@/i18n'
import Tasks from '@/pages/Tasks'
import * as api from '@/lib/tauri-api'

function wrap(ui: React.ReactElement, initialEntry: string | { pathname: string; state?: unknown } = '/tasks') {
  return (
    <I18nProvider>
      <AppProvider>
        <MemoryRouter initialEntries={[initialEntry]}>
          {ui}
        </MemoryRouter>
      </AppProvider>
    </I18nProvider>
  )
}

describe('Tasks page', () => {
  it('renders scheduled tasks heading', async () => {
    render(wrap(<Tasks />))
    await waitFor(() => expect(screen.getByText(/Create and monitor automations/)).toBeInTheDocument())
  })

  // IA T4: the only primary CTA is「新建自动化」; the one-off background
  // task entry folds into the split-button dropdown.
  it('renders the New primary CTA and folds New Background Task into its menu', async () => {
    render(wrap(<Tasks />))
    await waitFor(() => expect(screen.getByRole('button', { name: 'New' })).toBeInTheDocument())
    // Menu is closed — the secondary entry is not laid out flat.
    expect(screen.queryByRole('menuitem', { name: 'New Background Task' })).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'More ways to create' }))
    expect(await screen.findByRole('menuitem', { name: 'New Background Task' })).toBeInTheDocument()
    // Selecting it opens the NewTaskForm (one-off background task form).
    fireEvent.click(screen.getByRole('menuitem', { name: 'New Background Task' }))
    await waitFor(() => expect(screen.getByText('Create Background Task')).toBeInTheDocument())
  })

  // IA T2: the primary CTA opens the ScheduleForm (schedule an automation).
  it('opens ScheduleForm from the New CTA', async () => {
    render(wrap(<Tasks />))
    await waitFor(() => expect(screen.getByRole('button', { name: 'New' })).toBeInTheDocument())
    fireEvent.click(screen.getByRole('button', { name: 'New' }))
    await waitFor(() => expect(screen.getByText('Create Scheduled Routine')).toBeInTheDocument())
  })

  it('renders empty state when no tasks', async () => {
    render(wrap(<Tasks />))
    await waitFor(() => expect(screen.getByText('No tasks yet.')).toBeInTheDocument())
  })

  it('renders New task CTA in empty state', async () => {
    render(wrap(<Tasks />))
    await waitFor(() => expect(screen.getByText('New task')).toBeInTheDocument())
  })

  it('renders calendar schedule widget', async () => {
    render(wrap(<Tasks />))
    await waitFor(() => expect(screen.getByText('Schedule')).toBeInTheDocument())
  })

  it('renders task completion section', async () => {
    render(wrap(<Tasks />))
    await waitFor(() => expect(screen.getByText(/Task Completion/i)).toBeInTheDocument())
  })

  // IA T2 (互链闭环): a Triage card deep-links here with { openRoutineId }
  // in router state — the RoutineDetailDrawer must open for that routine
  // and the one-shot state must be drained.
  it('opens RoutineDetailDrawer from router state openRoutineId', async () => {
    vi.mocked(api.listScheduledTasks).mockResolvedValue([
      { id: 'r-42', name: 'Digest routine', prompt: 'Do the digest', trigger_type: 'cron', enabled: true },
    ] as any)
    render(
      wrap(<Tasks />, { pathname: '/tasks', state: { openRoutineId: 'r-42' } }),
    )
    const drawer = await screen.findByRole('dialog', { name: /Routine detail: Digest routine/i })
    expect(drawer).toBeInTheDocument()
  })

  it('does not open the routine drawer without router state', async () => {
    vi.mocked(api.listScheduledTasks).mockResolvedValue([
      { id: 'r-42', name: 'Digest routine', prompt: 'Do the digest', trigger_type: 'cron', enabled: true },
    ] as any)
    render(wrap(<Tasks />))
    await waitFor(() => expect(screen.getByText(/Create and monitor automations/)).toBeInTheDocument())
    expect(screen.queryByRole('dialog', { name: /Routine detail/i })).not.toBeInTheDocument()
  })

  // B4 裁决 (audit R1 §09 / R3-V-12): Simple mode has no sidebar entry for
  // the Mission Control — the tasks page carries the promised entry card.
  it('shows the Mission Control entry card in simple mode', async () => {
    render(wrap(<Tasks />))
    await waitFor(() => expect(screen.getByTestId('tasks-opc-entry')).toBeInTheDocument())
    expect(screen.getByRole('button', { name: 'Open Mission Control' })).toBeInTheDocument()
  })

  // Design 04 (audit R1 §04): a routine the SYSTEM paused leads its row with
  // the auto-pause badge and the「继续处理」(Resume) primary, joined through
  // the routine id the catalog card shares.
  it('shows the auto-paused badge and Resume primary on a system-paused routine', async () => {
    vi.mocked(api.listTasks).mockResolvedValue([
      { id: 'r-pause', title: 'Nightly digest', status: 'pending' },
    ] as any)
    vi.mocked(api.listScheduledTasks).mockResolvedValue([
      {
        id: 'r-pause',
        name: 'Nightly digest',
        prompt: 'digest',
        trigger_type: 'cron',
        enabled: false,
        pause_reason: 'consecutive_failures',
      },
    ] as any)
    render(wrap(<Tasks />))
    expect(await screen.findByTestId('task-card-auto-paused-badge')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Resume' })).toBeInTheDocument()
    expect(screen.getByText(/Auto-paused/)).toBeInTheDocument()
  })
})
