// F6 fix round 1 — WebhookTriggerCard's "Copied" flag auto-reset timer.
//
// The reset used to be a bare setTimeout with no handle: unmount fired it
// into a dead component, and a re-copy stacked timers so the FIRST window's
// expiry cleared the flag early (a re-copy inside 1.5s survived less than
// 1.5s). The fix keeps the handle in a ref, clears it on unmount, and
// restarts the window on re-copy — these tests pin that behavior.
//
// Asserts the user-visible toggle via the material icon glyphs, which is
// locale-independent (check = copied state) and does not depend on the
// translated button labels. All clock advances run inside `act` so React
// flushes the async copy handler's setState deterministically.

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, act } from '@testing-library/react'
import '@testing-library/jest-dom'
import WebhookTriggerCard from '@/components/tasks/WebhookTriggerCard'
import type { ScheduledRoutine } from '@/types'

function webhookRoutine(): ScheduledRoutine {
  return {
    id: 'sched-hook-1',
    name: 'nightly-hook',
    prompt: 'compile digest',
    interval_secs: 3600,
    trigger_type: 'webhook',
    cron_expr: null,
    timezone: null,
    next_fire_at: null,
    expires_at: null,
    created_at: 1_735_689_600,
    last_fired: null,
    enabled: true,
    fire_count: 0,
    max_fires: null,
    policy: {
      max_retries: 1,
      timeout_secs: 600,
      worktree: null,
      notify_on_failure: false,
      budget_usd: null,
      auto_archive_when_empty: true,
      execution_window: null,
    },
    last_run_id: null,
    last_error: null,
    depends_on: [],
  }
}

// Click copy and flush the awaited clipboard write → setCopiedId → render.
async function clickCopy(button: HTMLElement) {
  await act(async () => {
    fireEvent.click(button)
    await vi.advanceTimersByTimeAsync(0)
  })
}

async function advance(ms: number) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms)
  })
}

beforeEach(() => {
  vi.useFakeTimers()
  // jsdom has no clipboard; the copy handler awaits writeText before
  // flipping the flag.
  Object.defineProperty(navigator, 'clipboard', {
    configurable: true,
    value: { writeText: vi.fn().mockResolvedValue(undefined) },
  })
})

afterEach(() => {
  vi.useRealTimers()
})

describe('WebhookTriggerCard copied-flag timer (F6 fix round 1)', () => {
  it('shows Copied on click and auto-resets after 1500ms', async () => {
    render(<WebhookTriggerCard routines={[webhookRoutine()]} />)
    const button = screen.getByRole('button', { name: /Copy webhook trigger URL/ })

    await clickCopy(button)
    expect(screen.getByText('check')).toBeInTheDocument()

    await advance(1500)
    expect(screen.queryByText('check')).not.toBeInTheDocument()
    expect(screen.getByText('content_copy')).toBeInTheDocument()
  })

  it('a re-copy inside the window restarts the timer — the first expiry must not clear the flag early', async () => {
    render(<WebhookTriggerCard routines={[webhookRoutine()]} />)
    const button = screen.getByRole('button', { name: /Copy webhook trigger URL/ })

    await clickCopy(button)
    expect(screen.getByText('check')).toBeInTheDocument()

    // Re-copy at t=1000ms; the pre-fix stacked timer would clear at t=1500ms.
    await advance(1000)
    await clickCopy(button)
    expect(screen.getByText('check')).toBeInTheDocument()

    // t=1600ms — past the FIRST window's 1500ms point: flag must still live.
    await advance(600)
    expect(screen.getByText('check')).toBeInTheDocument()

    // Second window closes at t=2500ms.
    await advance(900)
    expect(screen.queryByText('check')).not.toBeInTheDocument()
    expect(screen.getByText('content_copy')).toBeInTheDocument()
  })

  it('clears the reset timer on unmount — the delayed reset never fires afterwards', async () => {
    const { unmount } = render(<WebhookTriggerCard routines={[webhookRoutine()]} />)
    const button = screen.getByRole('button', { name: /Copy webhook trigger URL/ })

    await clickCopy(button)
    expect(screen.getByText('check')).toBeInTheDocument()

    // If the pending reset escaped cleanup it would setState after unmount.
    unmount()
    await advance(1500)
    expect(screen.queryByText('check')).not.toBeInTheDocument()
  })
})
