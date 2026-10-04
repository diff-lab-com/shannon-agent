// B3-1 (P1-2, R9-②): sidebar queue badge. A session parked with queued
// prompts while another one is on screen keeps its FIFO (the drain only runs
// for the visible session), so the rail must carry the backlog count.
//  - the 「Queue {n}」chip renders only while the session holds a queue
//  - the count is the per-session depth (projection of AppContext's
//    promptQueues — fed here as the queueDepthsBySession prop)
//  - draining (depth disappears) clears the chip without touching other rows
//  - the chip coexists with the running dot (queue-while-streaming is the
//    scenario that produces a backlog)

import { describe, it, expect } from 'vitest'
import { render, screen, within } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { I18nProvider } from '@/i18n'
import { SessionsSection } from '@/components/SidebarSessions'
import type { SessionActivity, SessionInfo } from '@/types'

const NOW = Date.now()

function session(over: Partial<SessionInfo> = {}): SessionInfo {
  return { id: 's1', title: 'Session One', created_at: NOW - 3600_000, message_count: 2, ...over }
}

function activity(over: Partial<SessionActivity> = {}): SessionActivity {
  return { running: false, startedAt: null, lastActivity: NOW, activeTool: null, ...over }
}

const QUEUE_TITLE_2 = '2 messages will send when you return to this session'

function renderRail(props: {
  sessions: SessionInfo[]
  activity?: Record<string, SessionActivity>
  queueDepths?: Record<string, number>
}) {
  return render(
    <I18nProvider>
      <MemoryRouter>
        <SessionsSection
          sessions={props.sessions}
          sessionActivity={props.activity ?? {}}
          queueDepthsBySession={props.queueDepths}
          currentSessionId={null}
          switchSession={async () => {}}
          renameSession={async () => {}}
          deleteSession={async () => {}}
        />
      </MemoryRouter>
    </I18nProvider>,
  )
}

describe('session rail queue badge (B3-1)', () => {
  it('shows the queue chip with the parked count', () => {
    renderRail({ sessions: [session()], queueDepths: { s1: 2 } })
    expect(screen.getByText('Queue 2')).toBeInTheDocument()
    expect(screen.getByRole('img', { name: QUEUE_TITLE_2 })).toBeInTheDocument()
  })

  it('renders nothing when the session holds no queue', () => {
    renderRail({ sessions: [session()] }).unmount()
    renderRail({ sessions: [session()], queueDepths: { s1: 0 } })
    expect(screen.queryByRole('img', { name: /messages will send/ })).not.toBeInTheDocument()
  })

  it('scopes the badge to the queued session', () => {
    renderRail({
      sessions: [session(), session({ id: 's2', title: 'Session Two' })],
      queueDepths: { s1: 2 },
    })
    expect(screen.getAllByRole('img', { name: QUEUE_TITLE_2 })).toHaveLength(1)
    const otherRow = screen.getByTestId('desktop-session-row-s2')
    expect(within(otherRow).queryByText(/Queue \d/)).not.toBeInTheDocument()
  })

  it('clears once the queue drains and leaves other sessions untouched', () => {
    const view = renderRail({
      sessions: [session(), session({ id: 's2', title: 'Session Two' })],
      queueDepths: { s1: 2, s2: 1 },
    })
    // Drain s1's queue (the dequeue publishes a fresh depth record)…
    view.rerender(
      <I18nProvider>
        <MemoryRouter>
          <SessionsSection
            sessions={[session(), session({ id: 's2', title: 'Session Two' })]}
            sessionActivity={{}}
            queueDepthsBySession={{ s2: 1 }}
            currentSessionId={null}
            switchSession={async () => {}}
            renameSession={async () => {}}
            deleteSession={async () => {}}
          />
        </MemoryRouter>
      </I18nProvider>,
    )
    expect(screen.queryByRole('img', { name: QUEUE_TITLE_2 })).not.toBeInTheDocument()
    // …while s2's backlog stays visible with its own count.
    expect(screen.getByRole('img', { name: '1 messages will send when you return to this session' })).toBeInTheDocument()
  })

  it('coexists with the running dot (queue while streaming)', () => {
    renderRail({
      sessions: [session({ updated_at: NOW - 3 * 3600_000 })],
      activity: { s1: activity({ running: true, startedAt: NOW - 65_000 }) },
      queueDepths: { s1: 2 },
    })
    expect(screen.getByText('1m')).toBeInTheDocument()
    expect(screen.getByRole('img', { name: QUEUE_TITLE_2 })).toBeInTheDocument()
    expect(screen.getByText('Queue 2')).toBeInTheDocument()
  })
})
