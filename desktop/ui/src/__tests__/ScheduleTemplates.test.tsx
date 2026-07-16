// P3.2 ScheduleTemplates tests.

import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import ScheduleTemplates, { SCHEDULE_TEMPLATES } from '@/components/tasks/ScheduleTemplates'

describe('SCHEDULE_TEMPLATES catalog', () => {
  it('includes the expected starter set', () => {
    const ids = SCHEDULE_TEMPLATES.map(t => t.id)
    expect(ids).toContain('daily-standup')
    expect(ids).toContain('weekly-deps')
    expect(ids).toContain('pr-auto-review')
    expect(ids).toContain('changelog')
    expect(ids).toContain('nightly-tests')
  })
  it('every template carries required fields', () => {
    for (const t of SCHEDULE_TEMPLATES) {
      expect(t.name).toBeTruthy()
      expect(t.description).toBeTruthy()
      expect(t.icon).toBeTruthy()
      expect(t.fields.trigger_type).toMatch(/^(cron|interval|webhook|event)$/)
      if (t.fields.trigger_type === 'cron') expect(t.fields.cron_expr).toBeTruthy()
      if (t.fields.trigger_type === 'interval') expect(t.fields.interval_secs).toBeGreaterThan(0)
    }
  })
})

describe('ScheduleTemplates UI', () => {
  it('renders a chip for each template', () => {
    render(<ScheduleTemplates onApply={() => {}} />)
    for (const t of SCHEDULE_TEMPLATES) {
      expect(screen.getByText(t.name)).toBeInTheDocument()
    }
  })
  it('calls onApply with the clicked template', () => {
    const onApply = vi.fn()
    render(<ScheduleTemplates onApply={onApply} />)
    fireEvent.click(screen.getByText('Daily Standup Summary'))
    expect(onApply).toHaveBeenCalledTimes(1)
    const arg = onApply.mock.calls[0][0]
    expect(arg.id).toBe('daily-standup')
    expect(arg.fields.trigger_type).toBe('cron')
    expect(arg.fields.cron_expr).toBe('0 9 * * *')
  })
})
