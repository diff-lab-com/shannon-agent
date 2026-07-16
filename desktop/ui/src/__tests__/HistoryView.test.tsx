import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import HistoryView from '@/components/tasks/HistoryView'

const listExecutions = vi.hoisted(() => vi.fn())
const getDetail = vi.hoisted(() => vi.fn())

vi.mock('@/lib/tauri-api', () => ({
  default: {},
  listTaskExecutions: (...args: unknown[]) => listExecutions(...args),
  getExecutionDetail: (...args: unknown[]) => getDetail(...args),
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
})

describe('HistoryView', () => {
  it('renders loading state initially', async () => {
    listExecutions.mockReturnValue(new Promise(() => {}))
    render(<HistoryView />)
    // Skeletons render while loading
    expect(screen.queryByText('Daily Standup')).not.toBeInTheDocument()
  })

  it('renders empty state when no executions', async () => {
    listExecutions.mockResolvedValue([])
    render(<HistoryView />)
    await waitFor(() => expect(screen.getByText(/No execution history/)).toBeInTheDocument())
  })

  it('renders error state when fetch fails', async () => {
    listExecutions.mockRejectedValue(new Error('Network error'))
    render(<HistoryView />)
    await waitFor(() => expect(screen.getByText('Network error')).toBeInTheDocument())
  })

  it('renders execution rows with task name', async () => {
    listExecutions.mockResolvedValue([sampleRow])
    render(<HistoryView />)
    await waitFor(() => expect(screen.getByText('Daily Standup')).toBeInTheDocument())
    expect(screen.getByText(/Completed/)).toBeInTheDocument()
  })

  it('shows duration in minutes', async () => {
    listExecutions.mockResolvedValue([sampleRow])
    render(<HistoryView />)
    await waitFor(() => expect(screen.getByText('1m')).toBeInTheDocument())
  })

  it('expands row to show detail on click', async () => {
    listExecutions.mockResolvedValue([sampleRow])
    getDetail.mockResolvedValue({ ...sampleRow, prompt: 'Summarize today' })
    render(<HistoryView />)
    await waitFor(() => expect(screen.getByText('Daily Standup')).toBeInTheDocument())
    fireEvent.click(screen.getByRole('button', { name: /expand/i }))
    await waitFor(() => expect(screen.getByText('Summarize today')).toBeInTheDocument())
  })

  it('toggles expanded row closed on second click', async () => {
    listExecutions.mockResolvedValue([sampleRow])
    getDetail.mockResolvedValue({ ...sampleRow, prompt: 'p' })
    render(<HistoryView />)
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
    render(<HistoryView />)
    await waitFor(() => expect(screen.getByText('Daily Standup')).toBeInTheDocument())
    fireEvent.click(screen.getByRole('button', { name: /expand/i }))
    await waitFor(() => expect(screen.getByText('Boom')).toBeInTheDocument())
  })
})
