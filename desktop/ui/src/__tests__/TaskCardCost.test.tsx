// 看板金额 (批 1) — TaskCard's ledger-cost chip.
//
// Pins the honesty contract on the money figure: the chip renders only when
// the backend actually joined a `cost_usd` (ledger-attributed spend of the
// producing agent session) — hand-built/adhoc tasks and unseen sessions stay
// `undefined` and render NOTHING, never a "$–" placeholder or an estimate.

import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import TaskCard from '@/components/tasks/TaskCard'
import type { TaskItem } from '@/types'

function wrap(ui: React.ReactElement) {
  return <I18nProvider>{ui}</I18nProvider>
}

function taskFixture(overrides: Partial<TaskItem> = {}): TaskItem {
  return {
    id: 't1',
    title: 'Write release notes',
    status: 'completed',
    assignee: null,
    priority: null,
    description: null,
    team: null,
    ...overrides,
  }
}

function renderCard(task: TaskItem) {
  return render(
    wrap(
      <TaskCard task={task} isRunning={false} onSelect={() => {}} onRunNow={() => {}} onCancel={() => {}} />,
    ),
  )
}

describe('TaskCard 看板金额 — ledger cost chip (批 1)', () => {
  it('renders the joined ledger cost as two-decimal USD in the meta row', () => {
    renderCard(taskFixture({ cost_usd: 0.25 }))
    const chip = screen.getByTestId('task-card-cost')
    expect(chip).toHaveTextContent('$0.25')
    // 口径注记 lives on the tooltip: ledger spend of the producing session,
    // kept per ledger rotation — not a lifetime total.
    expect(chip.getAttribute('title')).toMatch(/[Ll]edger/)
  })

  it('renders a zero-spend session as $0.00 (Some(0.0) is real data)', () => {
    renderCard(taskFixture({ cost_usd: 0 }))
    expect(screen.getByTestId('task-card-cost')).toHaveTextContent('$0.00')
  })

  it('renders nothing when no cost was joined (never a "$–" placeholder)', () => {
    renderCard(taskFixture({ cost_usd: undefined }))
    expect(screen.queryByTestId('task-card-cost')).toBeNull()
  })
})
