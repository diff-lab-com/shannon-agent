import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import '@testing-library/jest-dom'
import * as api from '@/lib/tauri-api'
import CostEstimateHint from '@/components/tasks/CostEstimateHint'
import type { TaskCostEstimate } from '@/types'

function estimate(partial: Partial<TaskCostEstimate>): TaskCostEstimate {
  return {
    hasHistory: true,
    runsCounted: 5,
    minUsd: 0.1,
    maxUsd: 0.3,
    avgUsd: 0.2,
    lastUsd: 0.2,
    ...partial,
  }
}

describe('CostEstimateHint (P2-6)', () => {
  beforeEach(() => {
    vi.mocked(api.estimateTaskCost).mockReset()
  })

  it('renders nothing while the estimate is loading', () => {
    vi.mocked(api.estimateTaskCost).mockReturnValue(new Promise(() => {}))
    const { container } = render(<CostEstimateHint />)
    expect(container).toBeEmptyDOMElement()
  })

  it('shows the min-max range with avg and run count when history exists', async () => {
    vi.mocked(api.estimateTaskCost).mockResolvedValue(estimate({}))
    render(<CostEstimateHint />)

    await waitFor(() => {
      expect(screen.getByTestId('cost-estimate-hint')).toHaveTextContent('Est. $0.10–$0.30 per run')
    })
    expect(screen.getByTestId('cost-estimate-hint')).toHaveTextContent('avg $0.20')
    expect(screen.getByTestId('cost-estimate-hint')).toHaveTextContent('from 5 recent runs')
  })

  it('shows the first-run copy when there is no cost-tracked history', async () => {
    vi.mocked(api.estimateTaskCost).mockResolvedValue(estimate({ hasHistory: false, runsCounted: 0, minUsd: null, maxUsd: null, avgUsd: null, lastUsd: null }))
    render(<CostEstimateHint />)

    await waitFor(() => {
      expect(screen.getByTestId('cost-estimate-hint')).toHaveTextContent('First run — no estimate yet')
    })
  })

  it('passes the taskId through (null = all-routines baseline) and hides on failure', async () => {
    vi.mocked(api.estimateTaskCost).mockRejectedValue(new Error('bridge down'))
    const { container } = render(<CostEstimateHint taskId="routine-1" />)

    await waitFor(() => {
      expect(api.estimateTaskCost).toHaveBeenCalledWith('routine-1')
    })
    // A failed estimate stays invisible — never a broken-looking line.
    await waitFor(() => {
      expect(container).toBeEmptyDOMElement()
    })
  })
})
