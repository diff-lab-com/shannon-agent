import { describe, it, expect } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import TaskCalendarView from '@/components/tasks/TaskCalendarView'
import type { TaskItem } from '@/types'

// B3 P1-26: "tasks for the selected day" used to render the global list's
// first five rows regardless of the selection. These tests pin the
// due_date-based day filter and its dedicated empty state.

// Fixed "now"-ish view month so the calendar grid is deterministic.
const VIEW_YEAR = 2026
const VIEW_MONTH = 8 // September

function task(id: string, dueDay: number | null): TaskItem {
  return {
    id,
    title: `task-${id}`,
    status: 'pending',
    due_date: dueDay == null ? null : new Date(VIEW_YEAR, VIEW_MONTH, dueDay, 12).getTime() / 1000,
  }
}

function wrap(ui: React.ReactElement) {
  return <I18nProvider>{ui}</I18nProvider>
}

function renderCalendar(tasks: TaskItem[], selectedDay: number | null) {
  return render(
    wrap(
      <TaskCalendarView
        viewMonth={VIEW_MONTH}
        viewYear={VIEW_YEAR}
        selectedDay={selectedDay}
        filteredTasks={tasks}
        allTasks={tasks}
        agents={[]}
        efficiencyPct={0}
        onSelectDay={() => {}}
        onSelectTask={() => {}}
      />,
    ),
  )
}

describe('TaskCalendarView — selected-day filter (B3 P1-26)', () => {
  it('shows only tasks due on the selected day', () => {
    const tasks = [task('a', 15), task('b', 16), task('c', null)]
    const { container } = renderCalendar(tasks, 15)

    fireEvent.click(withinDay(container, 15))

    expect(screen.getByText('task-a')).toBeInTheDocument()
    expect(screen.queryByText('task-b')).not.toBeInTheDocument()
    // Tasks without a due date never belong to a calendar day.
    expect(screen.queryByText('task-c')).not.toBeInTheDocument()
  })

  it('shows the dedicated empty message when nothing is due that day', () => {
    const tasks = [task('b', 16)]
    const { container } = renderCalendar(tasks, 15)

    fireEvent.click(withinDay(container, 15))

    expect(screen.queryByText('task-b')).not.toBeInTheDocument()
    expect(screen.getByText(/no tasks due on this day/i)).toBeInTheDocument()
  })
})

/** Click inside a calendar day cell (the grid cells carry the day number). */
function withinDay(container: HTMLElement, day: number): HTMLElement {
  const cells = Array.from(container.querySelectorAll('.cursor-pointer'))
  const cell = cells.find(el => el.textContent?.trim().startsWith(String(day)))
  if (!cell) throw new Error(`day ${day} cell not found`)
  return cell as HTMLElement
}
