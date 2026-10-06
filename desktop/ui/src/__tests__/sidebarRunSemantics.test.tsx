// Batch B (2026-09-20 delta analysis §批B): sidebar run-monitor semantics.
//  - B1: idle rows carry a compact relative time-ago badge (updated_at)
//  - B2: amber dot while a permission prompt pends; red dot after a failed
//    run; green pulse still wins while running
//  - formatRelativeTime bucketing (now/min/hour/day/date degradation)

import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { I18nProvider } from '@/i18n'
import { SessionsSection, formatRelativeTime } from '@/components/SidebarSessions'
import type { SessionActivity, SessionInfo } from '@/types'

const NOW = Date.now()

function session(over: Partial<SessionInfo> = {}): SessionInfo {
  return { id: 's1', title: 'Session One', created_at: NOW - 3600_000, message_count: 2, ...over }
}

function activity(over: Partial<SessionActivity> = {}): SessionActivity {
  return { running: false, startedAt: null, lastActivity: NOW, activeTool: null, ...over }
}

function renderRail(props: { sessions: SessionInfo[]; activity?: Record<string, SessionActivity> }) {
  return render(
    <I18nProvider>
      <MemoryRouter>
        <SessionsSection
          sessions={props.sessions}
          sessionActivity={props.activity ?? {}}
          currentSessionId={null}
          switchSession={async () => {}}
          renameSession={async () => {}}
          deleteSession={async () => {}}
        />
      </MemoryRouter>
    </I18nProvider>,
  )
}

describe('formatRelativeTime (B1)', () => {
  const t = (id: string, values?: Record<string, number>) => {
    if (id.endsWith('.now')) return 'just now'
    if (id.endsWith('.minutes')) return `${values?.n}m`
    if (id.endsWith('.hours')) return `${values?.n}h`
    if (id.endsWith('.days')) return `${values?.n}d`
    return id
  }

  it('buckets sub-minute to now', () => {
    expect(formatRelativeTime(NOW - 30_000, NOW, t)).toBe('just now')
  })
  it('buckets minutes/hours/days', () => {
    expect(formatRelativeTime(NOW - 5 * 60_000, NOW, t)).toBe('5m')
    expect(formatRelativeTime(NOW - 17 * 3600_000, NOW, t)).toBe('17h')
    expect(formatRelativeTime(NOW - 12 * 24 * 3600_000, NOW, t)).toMatch(/^\d{1,2}\/\d{1,2}$/)
  })
  it('returns empty for missing/invalid timestamps', () => {
    expect(formatRelativeTime(undefined, NOW, t)).toBe('')
    expect(formatRelativeTime(0, NOW, t)).toBe('')
  })
})

describe('session rail run semantics (B1/B2)', () => {
  it('shows a time-ago badge on idle rows', () => {
    renderRail({ sessions: [session({ updated_at: NOW - 3 * 3600_000 })] })
    expect(screen.getByRole('img', { name: 'Last active 3h' })).toBeInTheDocument()
    expect(screen.getByText('3h')).toBeInTheDocument()
  })

  it('prefers the elapsed badge on running rows', () => {
    renderRail({
      sessions: [session({ updated_at: NOW - 3 * 3600_000 })],
      activity: { s1: activity({ running: true, startedAt: NOW - 65_000 }) },
    })
    expect(screen.getByText('1m')).toBeInTheDocument()
    expect(screen.queryByRole('img', { name: /Last active/ })).not.toBeInTheDocument()
  })

  it('shows the amber approval dot while a permission prompt pends', () => {
    renderRail({
      sessions: [session()],
      activity: { s1: activity({ awaitingApproval: true }) },
    })
    expect(screen.getByRole('img', { name: 'Waiting for approval' })).toBeInTheDocument()
  })

  it('shows the red failure dot after a failed run', () => {
    renderRail({
      sessions: [session()],
      activity: { s1: activity({ failed: true }) },
    })
    expect(screen.getByRole('img', { name: 'Last run failed' })).toBeInTheDocument()
  })

  it('the running pulse wins over idle signals', () => {
    renderRail({
      sessions: [session()],
      activity: { s1: activity({ running: true, startedAt: NOW, failed: true, awaitingApproval: true }) },
    })
    expect(screen.queryByRole('img', { name: 'Last run failed' })).not.toBeInTheDocument()
    expect(screen.queryByRole('img', { name: 'Waiting for approval' })).not.toBeInTheDocument()
  })
})
