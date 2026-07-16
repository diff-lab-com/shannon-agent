// OpcAnalyticsDashboard tests.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import OpcAnalyticsDashboard from '@/components/opc/OpcAnalyticsDashboard'
import type { OpcMetrics } from '@/types'

const getOpcMetrics = vi.hoisted(() => vi.fn())

vi.mock('@/lib/tauri-api', () => ({
  getOpcMetrics: (...args: unknown[]) => getOpcMetrics(...args),
}))

const sample: OpcMetrics = {
  total: 12,
  completion_rate: 50,
  by_status: [
    { status: 'completed', count: 6 },
    { status: 'in_progress', count: 4 },
    { status: 'pending', count: 2 },
  ],
  by_priority: [
    { priority: 'high', count: 4 },
    { priority: 'normal', count: 6 },
    { priority: 'low', count: 2 },
  ],
  by_assignee: [
    { assignee: 'research', total: 5, done: 3, in_progress: 2 },
    { assignee: 'cod', total: 7, done: 3, in_progress: 2 },
  ],
  daily: [
    { date: '2026-06-08', created: 2, completed: 1 },
    { date: '2026-06-09', created: 3, completed: 2 },
    { date: '2026-06-10', created: 0, completed: 0 },
    { date: '2026-06-11', created: 1, completed: 1 },
    { date: '2026-06-12', created: 2, completed: 0 },
    { date: '2026-06-13', created: 0, completed: 0 },
    { date: '2026-06-14', created: 1, completed: 0 },
  ],
}

beforeEach(() => {
  getOpcMetrics.mockReset()
  getOpcMetrics.mockResolvedValue(sample)
})

describe('OpcAnalyticsDashboard', () => {
  it('shows loading state initially', () => {
    getOpcMetrics.mockReturnValue(new Promise(() => {})) // never resolves
    render(<OpcAnalyticsDashboard />)
    expect(screen.getByText('Loading analytics…')).toBeInTheDocument()
  })

  it('shows error state when fetch rejects', async () => {
    getOpcMetrics.mockRejectedValue(new Error('boom'))
    render(<OpcAnalyticsDashboard />)
    expect(await screen.findByText(/boom/)).toBeInTheDocument()
  })

  it('renders stat cards on success', async () => {
    render(<OpcAnalyticsDashboard />)
    await waitFor(() => expect(getOpcMetrics).toHaveBeenCalledTimes(1))
    expect(await screen.findByText('Total tasks')).toBeInTheDocument()
    expect(screen.getByText('12')).toBeInTheDocument() // total
    expect(screen.getByText('50%')).toBeInTheDocument() // completion rate
    expect(screen.getAllByText('6').length).toBeGreaterThan(0) // done count (also appears in priority list)
  })

  it('renders daily chart bars with correct titles', async () => {
    render(<OpcAnalyticsDashboard />)
    await screen.findByText('Total tasks')
    // Created / completed tooltips — multiple days share counts, so getAllBy.
    expect(screen.getAllByTitle('Created: 2').length).toBeGreaterThan(0)
    expect(screen.getAllByTitle('Completed: 1').length).toBeGreaterThan(0)
    expect(screen.getAllByTitle('Created: 0').length).toBeGreaterThan(0)
  })

  it('renders status breakdown list', async () => {
    render(<OpcAnalyticsDashboard />)
    expect(await screen.findByText('completed')).toBeInTheDocument()
    expect(screen.getByText('in_progress')).toBeInTheDocument()
    expect(screen.getByText('pending')).toBeInTheDocument()
  })

  it('renders priority breakdown list', async () => {
    render(<OpcAnalyticsDashboard />)
    expect(await screen.findByText('high')).toBeInTheDocument()
    expect(screen.getByText('normal')).toBeInTheDocument()
    expect(screen.getByText('low')).toBeInTheDocument()
  })

  it('renders assignee workload list', async () => {
    render(<OpcAnalyticsDashboard />)
    expect(await screen.findByText('research')).toBeInTheDocument()
    expect(screen.getByText('cod')).toBeInTheDocument()
  })

  it('refresh button re-fetches metrics', async () => {
    render(<OpcAnalyticsDashboard />)
    await screen.findByText('Total tasks')
    fireEvent.click(screen.getByLabelText('Refresh analytics'))
    await waitFor(() => expect(getOpcMetrics).toHaveBeenCalledTimes(2))
  })

  it('renders empty messages when no data', async () => {
    const empty: OpcMetrics = {
      total: 0,
      completion_rate: 0,
      by_status: [],
      by_priority: [],
      by_assignee: [],
      daily: [],
    }
    getOpcMetrics.mockResolvedValue(empty)
    render(<OpcAnalyticsDashboard />)
    await waitFor(() => expect(getOpcMetrics).toHaveBeenCalled())
    expect(await screen.findByText(/No activity yet/i)).toBeInTheDocument()
    expect(screen.getByText(/No tasks/i)).toBeInTheDocument()
    expect(screen.getByText(/No priority set/i)).toBeInTheDocument()
    expect(screen.getByText(/No assignees/i)).toBeInTheDocument()
  })
})
