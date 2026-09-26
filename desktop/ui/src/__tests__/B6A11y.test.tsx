// B6-37 — behaviour tests for the a11y batch.
//
// Each case pins a change from review §7 item 37:
//   - AdvancedSettings switches carry accessible names (aria-label).
//   - TaskCard opens via a title button (container must stay non-interactive:
//     role="button" around real buttons is a nested-interactive violation);
//     TaskCalendarView day cells / selected-day rows are keyboard activatable.
//   - TaskCard status badge container announces politely (aria-live).
//   - Usage's mode toggle is an aria-pressed button pair (not a tablist).

import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import { AppProvider } from '@/context/AppContext'
import { MemoryRouter } from 'react-router-dom'
import TaskCard from '@/components/tasks/TaskCard'
import TaskCalendarView from '@/components/tasks/TaskCalendarView'
import Usage from '@/pages/Usage'
import * as api from '@/lib/tauri-api'
import type { TaskItem, UsageStats } from '@/types'

const wrap = (ui: React.ReactElement) => (
  <I18nProvider>
    <AppProvider>
      <MemoryRouter>{ui}</MemoryRouter>
    </AppProvider>
  </I18nProvider>
)

function taskFixture(overrides: Partial<TaskItem> = {}): TaskItem {
  return {
    id: 't1',
    title: 'Write release notes',
    status: 'pending',
    due_date: null,
    priority: null,
    assignee: null,
    team: null,
    description: null,
    execution_mode: 'serial',
    created_at: 0,
    updated_at: 0,
    ...overrides,
  } as TaskItem
}

describe('B6-37 Switch accessible names (AdvancedSettings)', () => {
  it('every settings switch resolves to a named role="switch"', async () => {
    const { default: AdvancedSettings } = await import('@/components/settings/AdvancedSettings')
    render(wrap(<AdvancedSettings />))
    const switches = await screen.findAllByRole('switch')
    expect(switches.length).toBeGreaterThanOrEqual(10)
    for (const s of switches) {
      expect(s).toHaveAttribute('aria-label')
      expect(s.getAttribute('aria-label')).not.toBe('')
    }
  })
})

describe('B6-37 TaskCard keyboard activation', () => {
  it('opens the task from the title button on click (card carries no role="button" — inner action buttons make a container button a nested-interactive axe violation)', () => {
    const onSelect = vi.fn()
    const { container } = render(wrap(
      <TaskCard task={taskFixture()} isRunning={false} onSelect={onSelect} onRunNow={() => {}} onCancel={() => {}} />,
    ))
    expect(container.querySelector('[role="button"]')).toBeNull()
    const titleButton = container.querySelector('h3 button') as HTMLButtonElement
    expect(titleButton).not.toBeNull()
    fireEvent.click(titleButton)
    expect(onSelect).toHaveBeenCalledTimes(1)
  })

  it('the status badge container announces politely', () => {
    const { container } = render(wrap(
      <TaskCard task={taskFixture()} isRunning={false} onSelect={() => {}} onRunNow={() => {}} onCancel={() => {}} />,
    ))
    const live = container.querySelector('[aria-live="polite"]')
    expect(live).not.toBeNull()
    expect(live?.getAttribute('title')).toBeTruthy()
  })
})

describe('B6-37 TaskCalendarView keyboard', () => {
  const VIEW_YEAR = 2026
  const VIEW_MONTH = 8 // September

  function task(id: string, dueDay: number): TaskItem {
    return {
      id,
      title: `task-${id}`,
      status: 'pending',
      due_date: new Date(VIEW_YEAR, VIEW_MONTH, dueDay, 12).getTime() / 1000,
    } as TaskItem
  }

  function calendar(onSelectDay: () => void, onSelectTask: (id: string) => void, selectedDay: number | null) {
    return render(wrap(
      <TaskCalendarView
        viewMonth={VIEW_MONTH}
        viewYear={VIEW_YEAR}
        selectedDay={selectedDay}
        filteredTasks={[task('a', 15)]}
        allTasks={[task('a', 15)]}
        agents={[]}
        efficiencyPct={0}
        onSelectDay={onSelectDay}
        onSelectTask={onSelectTask}
      />,
    ))
  }

  it('day cells are buttons and react to Enter/Space', () => {
    const onSelectDay = vi.fn()
    const { container } = calendar(onSelectDay, () => {}, null)
    const cell = container.querySelector('[role="button"][aria-label="Day 15"]') as HTMLElement
    expect(cell).not.toBeNull()
    fireEvent.keyDown(cell, { key: 'Enter' })
    fireEvent.keyDown(cell, { key: ' ' })
    expect(onSelectDay).toHaveBeenCalledTimes(2)
  })

  it('selected-day task rows are keyboard activatable', () => {
    const onSelectTask = vi.fn()
    calendar(() => {}, onSelectTask, 15)
    fireEvent.keyDown(screen.getByRole('button', { name: 'task-a' }), { key: 'Enter' })
    expect(onSelectTask).toHaveBeenCalledWith('a')
  })
})

describe('B6-37 Usage mode toggle', () => {
  const bucket = {
    input_tokens: 1000,
    output_tokens: 500,
    cache_creation_tokens: 100,
    cache_read_tokens: 50,
    cost_usd: 0.25,
    requests: 3,
  }
  const fixture: UsageStats = {
    days: 30,
    totals: { label: 'total', ...bucket },
    by_model: [{ label: 'claude-sonnet-4-6', ...bucket }],
    by_provider: [{ label: 'anthropic', ...bucket }],
    by_day: [{ label: '2024-01-02', ...bucket }],
  }

  it('renders aria-pressed buttons instead of a tablist', async () => {
    vi.mocked(api.getUsageStats).mockResolvedValue(fixture)
    render(wrap(<Usage />))
    const overview = await screen.findByRole('button', { name: /Overview/ })
    const audit = screen.getByRole('button', { name: /Audit \(table\)/ })
    expect(overview).toHaveAttribute('aria-pressed', 'true')
    expect(audit).toHaveAttribute('aria-pressed', 'false')
    expect(screen.queryByRole('tab')).not.toBeInTheDocument()
  })
})
