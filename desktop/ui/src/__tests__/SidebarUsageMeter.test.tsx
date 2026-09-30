import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor, fireEvent } from '@testing-library/react'
import '@testing-library/jest-dom'
import { MemoryRouter, useLocation } from 'react-router-dom'
import SidebarUsageMeter from '@/components/usage/SidebarUsageMeter'
import type { UsageGovernance } from '@/types'

// The meter reads the governance hook; mock it like the sidebar's inbox
// hook so tests drive the snapshot directly (no Tauri bridge involved).
const govMock = vi.hoisted(() => ({ current: null as UsageGovernance | null }))
vi.mock('@/hooks/useUsageGovernance', () => ({
  USAGE_GOVERNANCE_POLL_MS: 60_000,
  useUsageGovernance: () => ({ governance: govMock.current, refresh: vi.fn() }),
}))

function snapshot(partial: Partial<UsageGovernance>): UsageGovernance {
  return {
    month: '2026-10',
    monthCostUsd: 4.0,
    last7dCostUsd: 1.5,
    budgetUsd: 10.0,
    percent: 40,
    warned80: false,
    hit100: false,
    thresholdReached: null,
    ...partial,
  }
}

function LocationCapture() {
  const location = useLocation()
  return <div data-testid="current-location">{location.pathname}</div>
}

function wrap(ui: React.ReactElement) {
  return (
    <MemoryRouter initialEntries={['/chat']}>
      {ui}
      <LocationCapture />
    </MemoryRouter>
  )
}

describe('SidebarUsageMeter', () => {
  beforeEach(() => {
    govMock.current = null
  })

  it('renders nothing until the governance snapshot arrives', () => {
    render(wrap(<SidebarUsageMeter />))
    expect(screen.queryByTestId('sidebar-usage-meter')).not.toBeInTheDocument()
  })

  it('renders the % bar against the monthly budget', async () => {
    govMock.current = snapshot({ percent: 40 })
    render(wrap(<SidebarUsageMeter />))

    const meter = screen.getByTestId('sidebar-usage-meter')
    expect(meter).toBeInTheDocument()
    expect(screen.getByText('40% of monthly budget')).toBeInTheDocument()
    expect(screen.getByTestId('sidebar-usage-meter-fill')).toHaveStyle({ width: '40%' })
    // Hover explains the numbers + the reset date.
    expect(meter).toHaveAttribute('title', expect.stringContaining('$4.00 of $10.00'))
  })

  it('clamps the bar at 100% when spend bursts past the cap', () => {
    govMock.current = snapshot({ monthCostUsd: 14, percent: 140 })
    render(wrap(<SidebarUsageMeter />))
    expect(screen.getByTestId('sidebar-usage-meter-fill')).toHaveStyle({ width: '100%' })
  })

  it('falls back to the trailing 7-day cost without a budget', () => {
    govMock.current = snapshot({ budgetUsd: null, percent: null })
    render(wrap(<SidebarUsageMeter />))

    expect(screen.getByTestId('sidebar-usage-meter')).toBeInTheDocument()
    expect(screen.getByText('7-day spend $1.50')).toBeInTheDocument()
    expect(screen.queryByTestId('sidebar-usage-meter-fill')).not.toBeInTheDocument()
  })

  it('clicking the meter opens /usage', async () => {
    govMock.current = snapshot({})
    render(wrap(<SidebarUsageMeter />))

    fireEvent.click(screen.getByTestId('sidebar-usage-meter'))
    await waitFor(() => {
      expect(screen.getByTestId('current-location')).toHaveTextContent('/usage')
    })
  })
})
