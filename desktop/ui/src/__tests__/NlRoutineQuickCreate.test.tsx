// B2 NlRoutineQuickCreate — one-line NL card on the Tasks routines tab.
//
// Covers the four contract paths from the redesign brief:
//   parse success → structured preview renders (trigger / action / notify)
//   parse failure → honest guidance to the full form (never silent)
//   activate → onActivate receives the payload the preview showed
//   adjust → onAdjust hands the parsed values to ScheduleForm's prefill
// plus the pure splitter in ./nlQuickCreate (design example included) and
// the ScheduleForm side of the 调整 hand-off (initial-prop seeding).

import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import NlRoutineQuickCreate from '@/components/tasks/NlRoutineQuickCreate'
import { splitNlSchedule, deriveRoutineName, actionSnippet } from '@/components/tasks/nlQuickCreate'
import ScheduleForm from '@/components/tasks/ScheduleForm'
import { DEFAULT_POLICY } from '@/components/tasks/shared'
import type { CreateTaskPayload } from '@/types'

function renderCard(overrides: Partial<Parameters<typeof NlRoutineQuickCreate>[0]> = {}) {
  const onActivate = vi.fn().mockResolvedValue(true)
  const onAdjust = vi.fn()
  const onOpenForm = vi.fn()
  const utils = render(
    <NlRoutineQuickCreate
      onActivate={overrides.onActivate ?? onActivate}
      onAdjust={overrides.onAdjust ?? onAdjust}
      onOpenForm={overrides.onOpenForm ?? onOpenForm}
    />,
  )
  return { onActivate: overrides.onActivate ?? onActivate, onAdjust: overrides.onAdjust ?? onAdjust, onOpenForm: overrides.onOpenForm ?? onOpenForm, ...utils }
}

const type = (text: string) => {
  fireEvent.change(screen.getByLabelText('Describe the automation in one line'), { target: { value: text } })
}
const parse = () => fireEvent.click(screen.getByRole('button', { name: 'Parse' }))

describe('NlRoutineQuickCreate — parse success → preview', () => {
  it('renders trigger / action / notify chips from an English one-liner', () => {
    renderCard()
    type('weekdays at 9am, summarize merged PRs')
    parse()
    const preview = screen.getByTestId('nl-preview')
    expect(preview).toBeInTheDocument()
    // Trigger chip: localized schedule + raw cron, never just the cron.
    expect(screen.getByText('Weekdays at 09:00')).toBeInTheDocument()
    expect(screen.getByText('0 9 * * 1-5')).toBeInTheDocument()
    // Action chip: the user's own words, verbatim.
    expect(screen.getByText('summarize merged PRs')).toBeInTheDocument()
    // Notify chip: honest muted fallback, not a fabricated target.
    expect(screen.getByText('Not set — add it after creating')).toBeInTheDocument()
    // Structured-preview badge + both exits enabled.
    expect(screen.getByText('Structured preview')).toBeInTheDocument()
    expect(screen.getByTestId('nl-activate')).toBeEnabled()
    expect(screen.getByTestId('nl-adjust')).toBeEnabled()
  })

  it('renders the design example (zh run-on sentence) with the Discord target kept in the action', () => {
    renderCard()
    type('工作日早上 9 点汇总昨日 PR,发到 Discord')
    parse()
    expect(screen.getByText('Weekdays at 09:00')).toBeInTheDocument()
    expect(screen.getByText('0 9 * * 1-5')).toBeInTheDocument()
    // 发到 Discord is NOT a notification chip — it stays in the action text.
    expect(screen.getByText('汇总昨日 PR,发到 Discord')).toBeInTheDocument()
    expect(screen.getByText('Not set — add it after creating')).toBeInTheDocument()
  })

  it('Enter key parses', () => {
    renderCard()
    fireEvent.change(screen.getByLabelText('Describe the automation in one line'), { target: { value: 'daily at 9:30' } })
    fireEvent.keyDown(screen.getByLabelText('Describe the automation in one line'), { key: 'Enter' })
    expect(screen.getByText('Daily at 09:30')).toBeInTheDocument()
  })

  it('Parse stays disabled on empty input', () => {
    renderCard()
    expect(screen.getByRole('button', { name: 'Parse' })).toBeDisabled()
  })
})

describe('NlRoutineQuickCreate — parse failure → honest guidance', () => {
  it('shows the parse-failed guidance and opens the full form from it', () => {
    const { onOpenForm } = renderCard()
    type('summarize merged PRs every day at 9am') // action-first: not supported, honestly refused
    parse()
    expect(screen.getByTestId('nl-parse-failed')).toBeInTheDocument()
    expect(screen.getByText(/Couldn't read a schedule from that/)).toBeInTheDocument()
    // No preview → no activate/adjust exits at all (nothing to confirm).
    expect(screen.queryByTestId('nl-activate')).not.toBeInTheDocument()
    expect(screen.queryByTestId('nl-adjust')).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Open the full form' }))
    expect(onOpenForm).toHaveBeenCalledTimes(1)
    // No preview is fabricated for input the parser could not read.
    expect(screen.queryByTestId('nl-preview')).not.toBeInTheDocument()
  })

  it('recovers: a parseable retry clears the failure state', () => {
    renderCard()
    type('do the thing')
    parse()
    expect(screen.getByTestId('nl-parse-failed')).toBeInTheDocument()
    type('every 15 minutes')
    parse()
    expect(screen.queryByTestId('nl-parse-failed')).not.toBeInTheDocument()
    expect(screen.getByTestId('nl-preview')).toBeInTheDocument()
  })
})

describe('NlRoutineQuickCreate — activate → create call', () => {
  it('hands the page exactly what the preview showed', async () => {
    const { onActivate } = renderCard()
    type('weekdays at 9am, summarize merged PRs')
    parse()
    fireEvent.click(screen.getByTestId('nl-activate'))
    await waitFor(() => expect(onActivate).toHaveBeenCalledTimes(1))
    const payload = onActivate.mock.calls[0][0] as CreateTaskPayload
    expect(payload).toMatchObject({
      name: 'summarize merged PRs',
      prompt: 'summarize merged PRs',
      trigger_type: 'cron',
      cron_expr: '0 9 * * 1-5',
      notify_webhook: false,
      // The card promises the default policy — same one the form seeds.
      policy: DEFAULT_POLICY,
    })
  })

  it('success clears the card for the next one-liner', async () => {
    renderCard()
    type('daily at noon')
    parse()
    fireEvent.click(screen.getByTestId('nl-activate'))
    await waitFor(() => expect(screen.queryByTestId('nl-preview')).not.toBeInTheDocument())
    expect(screen.getByLabelText('Describe the automation in one line')).toHaveValue('')
  })

  it('failure keeps the preview and guides to the adjust path', async () => {
    const onActivate = vi.fn().mockResolvedValue(false)
    renderCard({ onActivate })
    type('daily at noon')
    parse()
    fireEvent.click(screen.getByTestId('nl-activate'))
    await waitFor(() => expect(screen.getByTestId('nl-activate-failed')).toBeInTheDocument())
    expect(screen.getByText(/Activation failed/)).toBeInTheDocument()
    // The preview survives so 调整 still has something to hand over.
    expect(screen.getByTestId('nl-preview')).toBeInTheDocument()
    expect(screen.getByTestId('nl-adjust')).toBeEnabled()
  })
})

describe('NlRoutineQuickCreate — adjust → form prefill hand-off', () => {
  it('passes derived name, prompt and cron to the page', () => {
    const { onAdjust } = renderCard()
    type('weekdays at 9am, summarize merged PRs')
    parse()
    fireEvent.click(screen.getByTestId('nl-adjust'))
    expect(onAdjust).toHaveBeenCalledWith({
      name: 'summarize merged PRs',
      prompt: 'summarize merged PRs',
      cronExpr: '0 9 * * 1-5',
    })
  })

  it('ScheduleForm seeds its edit step from the hand-off (initial prop)', () => {
    render(
      <ScheduleForm
        initial={{ name: 'summarize merged PRs', prompt: 'summarize merged PRs', triggerType: 'cron', cronExpr: '0 9 * * 1-5' }}
        onSubmit={() => {}}
        onCancel={() => {}}
      />,
    )
    expect(screen.getByPlaceholderText('e.g. Daily standup summary')).toHaveValue('summarize merged PRs')
    expect(screen.getByPlaceholderText('Describe what this routine should do...')).toHaveValue('summarize merged PRs')
    expect(screen.getByPlaceholderText('0 9 * * *')).toHaveValue('0 9 * * 1-5')
    // Trigger pre-selected as cron (the radio group reflects it).
    expect(screen.getByRole('radio', { name: /Cron/ })).toBeChecked()
  })
})

describe('nlQuickCreate — pure splitter', () => {
  it('design example: run-on zh sentence cuts at the confirmed time clause', () => {
    const hit = splitNlSchedule('工作日早上 9 点汇总昨日 PR,发到 Discord')
    expect(hit).not.toBeNull()
    expect(hit!.scheduleText).toBe('工作日早上 9 点')
    expect(hit!.restText).toBe('汇总昨日 PR,发到 Discord')
    expect(hit!.parsed.expression).toBe('0 9 * * 1-5')
  })

  it('comma-separated zh sentence splits on segments', () => {
    const hit = splitNlSchedule('每天早上 9 点,汇总昨日合并的 PR')
    expect(hit!.scheduleText).toBe('每天早上 9 点')
    expect(hit!.restText).toBe('汇总昨日合并的 PR')
    expect(hit!.parsed.expression).toBe('0 9 * * *')
  })

  it('a whole-string schedule leaves the action half empty', () => {
    const hit = splitNlSchedule('every 15 minutes')
    expect(hit!.scheduleText).toBe('every 15 minutes')
    expect(hit!.restText).toBe('')
    expect(hit!.parsed.expression).toBe('*/15 * * * *')
  })

  it('zh monthly-day clause cuts before the action', () => {
    const hit = splitNlSchedule('每月 15 号发周报')
    expect(hit!.scheduleText).toBe('每月 15 号')
    expect(hit!.restText).toBe('发周报')
    expect(hit!.parsed.expression).toBe('0 9 15 * *')
  })

  it('english run-on with a leading schedule clause splits after the time', () => {
    const hit = splitNlSchedule('daily at 9am summarize merged PRs')
    expect(hit!.scheduleText).toBe('daily at 9am')
    expect(hit!.restText).toBe('summarize merged PRs')
    expect(hit!.parsed.expression).toBe('0 9 * * *')
  })

  it('action-first sentences are honestly refused, never guessed', () => {
    expect(splitNlSchedule('summarize merged PRs every day at 9am')).toBeNull()
    expect(splitNlSchedule('do the thing')).toBeNull()
    expect(splitNlSchedule('')).toBeNull()
  })

  it('deriveRoutineName prefers the action half, truncates deterministically', () => {
    expect(deriveRoutineName('工作日早上 9 点', '汇总昨日 PR')).toBe('汇总昨日 PR')
    expect(deriveRoutineName('every 15 minutes', '')).toBe('every 15 minutes')
    expect(deriveRoutineName('s', '发周报。')).toBe('发周报')
    expect(deriveRoutineName('s', 'a'.repeat(40))).toBe(`${'a'.repeat(32)}…`)
  })

  it('actionSnippet collapses whitespace and truncates with an ellipsis', () => {
    expect(actionSnippet('  a   b  ')).toBe('a b')
    expect(actionSnippet('a'.repeat(48))).toHaveLength(48)
    expect(actionSnippet('a'.repeat(49))).toBe(`${'a'.repeat(48)}…`)
  })
})
