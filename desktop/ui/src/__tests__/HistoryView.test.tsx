import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { MemoryRouter, useLocation } from 'react-router-dom'
import HistoryView from '@/components/tasks/HistoryView'

const listExecutions = vi.hoisted(() => vi.fn())
const getDetail = vi.hoisted(() => vi.fn())
const listInboxItems = vi.hoisted(() => vi.fn())

vi.mock('@/lib/tauri-api', () => ({
  default: {},
  listTaskExecutions: (...args: unknown[]) => listExecutions(...args),
  getExecutionDetail: (...args: unknown[]) => getDetail(...args),
  listInboxItems: (...args: unknown[]) => listInboxItems(...args),
}))

const sampleRow = {
  run_id: 'run-1',
  task_id: 'task-1',
  task_name: 'Daily Standup',
  started_at: 1717000000,
  finished_at: 1717000060,
  status: 'completed',
  cost_usd: 0.0123,
  token_usage: 4500,
}

beforeEach(() => {
  listExecutions.mockReset()
  getDetail.mockReset()
  listInboxItems.mockReset()
  listInboxItems.mockResolvedValue([])
})

// HistoryView navigates to /triage (IA T2), so mount it inside a router
// with a probe exposing pathname + the handed-over state.
function LocationProbe() {
  const location = useLocation()
  return (
    <div
      data-testid="history-location"
      data-path={location.pathname}
      data-state={JSON.stringify(location.state ?? null)}
    />
  )
}

function renderView(ui: React.ReactElement = <HistoryView />, initialEntry = '/tasks') {
  return render(
    <MemoryRouter initialEntries={[initialEntry]}>
      {ui}
      <LocationProbe />
    </MemoryRouter>
  )
}

describe('HistoryView', () => {
  it('renders loading state initially', async () => {
    listExecutions.mockReturnValue(new Promise(() => {}))
    renderView()
    // Skeletons render while loading
    expect(screen.queryByText('Daily Standup')).not.toBeInTheDocument()
  })

  it('renders empty state when no executions', async () => {
    listExecutions.mockResolvedValue([])
    renderView()
    await waitFor(() => expect(screen.getByText(/No execution history/)).toBeInTheDocument())
  })

  it('renders error state when fetch fails', async () => {
    listExecutions.mockRejectedValue(new Error('Network error'))
    renderView()
    await waitFor(() => expect(screen.getByText('Network error')).toBeInTheDocument())
  })

  it('renders execution rows with task name', async () => {
    listExecutions.mockResolvedValue([sampleRow])
    renderView()
    await waitFor(() => expect(screen.getByText('Daily Standup')).toBeInTheDocument())
    expect(screen.getByText(/Completed/)).toBeInTheDocument()
  })

  it('shows duration in minutes', async () => {
    listExecutions.mockResolvedValue([sampleRow])
    renderView()
    await waitFor(() => expect(screen.getByText('1m')).toBeInTheDocument())
  })

  it('expands row to show detail on click', async () => {
    listExecutions.mockResolvedValue([sampleRow])
    getDetail.mockResolvedValue({ ...sampleRow, prompt: 'Summarize today' })
    renderView()
    await waitFor(() => expect(screen.getByText('Daily Standup')).toBeInTheDocument())
    fireEvent.click(screen.getByRole('button', { name: /expand/i }))
    await waitFor(() => expect(screen.getByText('Summarize today')).toBeInTheDocument())
  })

  it('toggles expanded row closed on second click', async () => {
    listExecutions.mockResolvedValue([sampleRow])
    getDetail.mockResolvedValue({ ...sampleRow, prompt: 'p' })
    renderView()
    await waitFor(() => expect(screen.getByText('Daily Standup')).toBeInTheDocument())
    const row = screen.getByRole('button', { name: /expand/i })
    fireEvent.click(row)
    await waitFor(() => expect(screen.getByText('p')).toBeInTheDocument())
    fireEvent.click(row)
    await waitFor(() => expect(screen.queryByText('p')).not.toBeInTheDocument())
  })

  it('shows error message in detail when row has error_message', async () => {
    listExecutions.mockResolvedValue([{ ...sampleRow, status: 'failed', error_message: 'Boom' }])
    getDetail.mockResolvedValue({ ...sampleRow, status: 'failed', error_message: 'Boom' })
    renderView()
    await waitFor(() => expect(screen.getByText('Daily Standup')).toBeInTheDocument())
    fireEvent.click(screen.getByRole('button', { name: /expand/i }))
    await waitFor(() => expect(screen.getByText('Boom')).toBeInTheDocument())
  })
})

// IA T2 (互链闭环): every execution row carries a「View in inbox」secondary
// action. When the run's task_id matches an inbox item's sourceId, the jump
// hands over { highlightInboxId } so Triage can ring that card; otherwise it
// degrades to the plain /triage list.
describe('HistoryView — View in inbox link (IA T2)', () => {
  it('navigates to /triage with highlight state when source_id matches an inbox item', async () => {
    listExecutions.mockResolvedValue([sampleRow])
    listInboxItems.mockResolvedValue([
      { id: 7, source: 'routine', sourceId: 'task-1', title: 'Digest finished', status: 'pending' },
      { id: 9, source: 'routine', sourceId: 'other-task', title: 'Unrelated', status: 'read' },
    ])
    renderView()
    await waitFor(() => expect(screen.getByText('Daily Standup')).toBeInTheDocument())
    fireEvent.click(screen.getByRole('button', { name: 'View this run in the Triage inbox' }))
    const probe = screen.getByTestId('history-location')
    expect(probe.getAttribute('data-path')).toBe('/triage')
    expect(JSON.parse(probe.getAttribute('data-state')!)).toEqual({ highlightInboxId: 7 })
  })

  it('navigates to the plain /triage list when no inbox item matches', async () => {
    listExecutions.mockResolvedValue([sampleRow])
    listInboxItems.mockResolvedValue([
      { id: 9, source: 'routine', sourceId: 'other-task', title: 'Unrelated', status: 'read' },
    ])
    renderView()
    await waitFor(() => expect(screen.getByText('Daily Standup')).toBeInTheDocument())
    fireEvent.click(screen.getByRole('button', { name: 'View this run in the Triage inbox' }))
    const probe = screen.getByTestId('history-location')
    expect(probe.getAttribute('data-path')).toBe('/triage')
    expect(JSON.parse(probe.getAttribute('data-state')!)).toBeNull()
  })

  it('keeps the expand toggle and the inbox link independent', async () => {
    listExecutions.mockResolvedValue([sampleRow])
    getDetail.mockResolvedValue({ ...sampleRow, prompt: 'Summarize today' })
    renderView()
    await waitFor(() => expect(screen.getByText('Daily Standup')).toBeInTheDocument())
    // Opening the detail does not navigate…
    fireEvent.click(screen.getByRole('button', { name: /expand/i }))
    await waitFor(() => expect(screen.getByText('Summarize today')).toBeInTheDocument())
    expect(screen.getByTestId('history-location').getAttribute('data-path')).toBe('/tasks')
    // …and the inbox link stays available next to the row.
    expect(screen.getByRole('button', { name: 'View this run in the Triage inbox' })).toBeInTheDocument()
  })
})
