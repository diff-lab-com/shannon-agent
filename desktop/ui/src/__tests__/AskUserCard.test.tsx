// Settings R3 T8 — the AskUserCard dialog: the GUI answer surface for the
// engine's `ask_user_question` tool (previously stdin-only → dead under a
// GUI, R8).
//
// Harness mirrors AppContextPermissionLifecycle.test.tsx: a captured
// @tauri-apps/api/event fake — events are flushed straight at the captured
// listeners (listener-level seam). `api.respondAskUser` is the setup.ts
// module-mock spy.

import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, screen, fireEvent, waitFor, within } from '@testing-library/react'
import AskUserCard from '@/components/chat/AskUserCard'
import { SessionContext, type SessionContextValue } from '@/context/SessionContext'
import { EVENT_NAMES } from '@/types'
import * as api from '@/lib/tauri-api'

const { captured, flush } = vi.hoisted(() => {
  const captured: Record<string, ((e: { payload: unknown }) => void)[]> = {}
  const flush = (event: string, payload: unknown) => {
    for (const h of captured[event] ?? []) h(payload)
  }
  return { captured, flush }
})

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn((event: string, handler: (e: { payload: unknown }) => void) => {
    captured[event] ??= []
    captured[event].push((payload) => handler({ payload }))
    return Promise.resolve(() => {
      captured[event] = (captured[event] ?? []).filter(h => h !== handler)
    })
  }),
  emit: vi.fn(),
}))

const request = (over: Partial<Record<string, unknown>> = {}) => ({
  request_id: 'req-1',
  question: 'Deploy to production?',
  header: 'Confirm',
  options: [
    { label: 'Yes', description: 'Ship it' },
    { label: 'No', description: '' },
  ],
  multi_select: false,
  ...over,
})

// F2: the card scopes itself via the session slice — provide a minimal
// SessionContext (AppProvider hosts the real provider in the app shell).
const baseSessionCtx: SessionContextValue = {
  sessions: [],
  sessionActivity: {},
  goalRunsBySession: {},
  subagentLive: null,
  currentSessionId: null,
  windowSessionId: null,
  switchingSession: false,
  createSession: vi.fn(async () => {}),
  createSessionInWorktree: vi.fn(async () => {}),
  switchSession: vi.fn(async () => {}),
  deleteSession: vi.fn(async () => {}),
  renameSession: vi.fn(async () => {}),
  refreshSessions: vi.fn(async () => {}),
  sessionSources: {},
  addSessionSource: vi.fn(),
  removeSessionSource: vi.fn(),
  queueDepthsBySession: {},
}

beforeEach(() => {
  vi.clearAllMocks()
  for (const key of Object.keys(captured)) delete captured[key]
  vi.mocked(api.respondAskUser).mockResolvedValue(undefined)
})

async function renderCard(session: Partial<SessionContextValue> = {}) {
  render(
    <SessionContext.Provider value={{ ...baseSessionCtx, ...session }}>
      <AskUserCard />
    </SessionContext.Provider>,
  )
  // The card renders nothing until listeners registered + a request arrived.
  await waitFor(() => {
    expect((captured[EVENT_NAMES.ASK_USER_REQUEST] ?? []).length).toBeGreaterThan(0)
  })
}

describe('AskUserCard (Settings R3 T8 — desktop ask_user round-trip)', () => {
  it('renders the question with options and answers a single-select click', async () => {
    await renderCard()
    flush(EVENT_NAMES.ASK_USER_REQUEST, request())

    expect(await screen.findByTestId('ask-user-card')).toBeInTheDocument()
    expect(screen.getByTestId('ask-user-question')).toHaveTextContent('Deploy to production?')

    const group = screen.getByTestId('ask-user-options')
    const yes = within(group).getByRole('button', { name: 'Yes' })
    fireEvent.click(yes)

    await waitFor(() => {
      expect(api.respondAskUser).toHaveBeenCalledWith('req-1', ['Yes'])
    })
    // Submitted state is visible ("Answered"), the form is gone.
    expect(await screen.findByTestId('ask-user-status')).toHaveTextContent('Answered')
    expect(screen.queryByTestId('ask-user-send')).not.toBeInTheDocument()
  })

  it('sends the typed free-text answer through the send button', async () => {
    await renderCard()
    flush(EVENT_NAMES.ASK_USER_REQUEST, request({ options: [], header: '' }))

    const input = await screen.findByTestId('ask-user-input')
    fireEvent.change(input, { target: { value: 'Ship on Thursday' } })
    fireEvent.click(screen.getByTestId('ask-user-send'))

    await waitFor(() => {
      expect(api.respondAskUser).toHaveBeenCalledWith('req-1', ['Ship on Thursday'])
    })
  })

  it('supports multi-select: toggles options, sends only on the send button', async () => {
    await renderCard()
    flush(EVENT_NAMES.ASK_USER_REQUEST, request({ multi_select: true }))

    const group = await screen.findByTestId('ask-user-options')
    const yes = within(group).getByRole('button', { name: 'Yes' })
    const no = within(group).getByRole('button', { name: 'No' })
    // Multi-select clicks toggle instead of submitting.
    fireEvent.click(yes)
    fireEvent.click(no)
    expect(yes).toHaveAttribute('aria-pressed', 'true')
    expect(no).toHaveAttribute('aria-pressed', 'true')
    expect(api.respondAskUser).not.toHaveBeenCalled()

    fireEvent.click(screen.getByTestId('ask-user-send'))
    await waitFor(() => {
      expect(api.respondAskUser).toHaveBeenCalledWith('req-1', ['Yes', 'No'])
    })
  })

  it('shows the timeout state when the auto-continue resolved event fires', async () => {
    await renderCard()
    flush(EVENT_NAMES.ASK_USER_REQUEST, request({ timeout_ms: 300000 }))
    await screen.findByTestId('ask-user-card')

    flush(EVENT_NAMES.ASK_USER_RESOLVED, { request_id: 'req-1', timed_out: true })

    const status = await screen.findByTestId('ask-user-status')
    expect(status).toHaveTextContent(/Timed out/i)
    expect(api.respondAskUser).not.toHaveBeenCalled()
  })

  it('ignores a resolved event for another request', async () => {
    await renderCard()
    flush(EVENT_NAMES.ASK_USER_REQUEST, request())
    await screen.findByTestId('ask-user-card')

    flush(EVENT_NAMES.ASK_USER_RESOLVED, { request_id: 'other-req', timed_out: true })

    expect(screen.queryByTestId('ask-user-status')).not.toBeInTheDocument()
    expect(screen.getByTestId('ask-user-send')).toBeInTheDocument()
  })

  it('shows the countdown only when the request carries a timeout (auto-continue on)', async () => {
    await renderCard()
    flush(EVENT_NAMES.ASK_USER_REQUEST, request({ timeout_ms: 300000 }))
    const countdown = await screen.findByTestId('ask-user-countdown')
    expect(countdown).toHaveTextContent('5:00')
    expect(countdown).toHaveAttribute('aria-label', 'Auto-continue in 5:00')

    // No timeout_ms (switch off → wait forever) → no countdown.
    flush(EVENT_NAMES.ASK_USER_REQUEST, request({ request_id: 'req-2' }))
    await screen.findByTestId('ask-user-question')
    await waitFor(() => {
      expect(screen.getByTestId('ask-user-question').textContent).not.toBe('')
    })
    // The new request replaced the card; the old countdown is gone.
    expect(screen.queryByTestId('ask-user-countdown')).not.toBeInTheDocument()
  })

  it('restores the pending form when the submit fails', async () => {
    vi.mocked(api.respondAskUser).mockRejectedValueOnce(new Error('ipc down'))
    await renderCard()
    flush(EVENT_NAMES.ASK_USER_REQUEST, request())

    const group = await screen.findByTestId('ask-user-options')
    fireEvent.click(within(group).getByRole('button', { name: 'Yes' }))
    await waitFor(() => expect(api.respondAskUser).toHaveBeenCalled())

    // Back to the form so the user can retry.
    expect(await screen.findByTestId('ask-user-send')).toBeInTheDocument()
  })

  // === F2 — session scoping + resolved broadcast on both paths ===

  it('shows a scoped request whose session matches the visible session', async () => {
    await renderCard({ currentSessionId: 'sess-a' })
    flush(EVENT_NAMES.ASK_USER_REQUEST, request({ session_id: 'sess-a' }))

    expect(await screen.findByTestId('ask-user-card')).toBeInTheDocument()
  })

  it('shows a scoped request matching the pinned window session (window mode)', async () => {
    await renderCard({ currentSessionId: null, windowSessionId: 'sess-w' })
    flush(EVENT_NAMES.ASK_USER_REQUEST, request({ session_id: 'sess-w' }))

    expect(await screen.findByTestId('ask-user-card')).toBeInTheDocument()
  })

  it('hides a scoped request for another session', async () => {
    await renderCard({ currentSessionId: 'sess-a' })
    flush(EVENT_NAMES.ASK_USER_REQUEST, request({ session_id: 'sess-b' }))

    // Background session's question (F2): scoped away from this window.
    expect(screen.queryByTestId('ask-user-card')).not.toBeInTheDocument()
    expect(screen.queryByTestId('ask-user-question')).not.toBeInTheDocument()
  })

  it('keeps a hidden scoped request hidden until its session becomes visible', async () => {
    const view = render(
      <SessionContext.Provider value={{ ...baseSessionCtx, currentSessionId: 'sess-a' }}>
        <AskUserCard />
      </SessionContext.Provider>,
    )
    await waitFor(() => {
      expect((captured[EVENT_NAMES.ASK_USER_REQUEST] ?? []).length).toBeGreaterThan(0)
    })
    flush(EVENT_NAMES.ASK_USER_REQUEST, request({ session_id: 'sess-b' }))
    expect(screen.queryByTestId('ask-user-card')).not.toBeInTheDocument()

    // User switches to the asking session → the pending card appears.
    view.rerender(
      <SessionContext.Provider value={{ ...baseSessionCtx, currentSessionId: 'sess-b' }}>
        <AskUserCard />
      </SessionContext.Provider>,
    )
    expect(await screen.findByTestId('ask-user-card')).toBeInTheDocument()
    expect(screen.getByTestId('ask-user-question')).toHaveTextContent('Deploy to production?')
  })

  it('shows an unscoped request (ambiguous run) regardless of the visible session', async () => {
    await renderCard({ currentSessionId: 'sess-a' })
    flush(EVENT_NAMES.ASK_USER_REQUEST, request()) // no session_id → every-window fallback

    expect(await screen.findByTestId('ask-user-card')).toBeInTheDocument()
  })

  it('clears a pending card when the answered broadcast (timed_out: false) arrives', async () => {
    await renderCard({ currentSessionId: 'sess-a' })
    flush(EVENT_NAMES.ASK_USER_REQUEST, request({ session_id: 'sess-a' }))
    await screen.findByTestId('ask-user-card')

    // Another window answered → every other window's card clears.
    flush(EVENT_NAMES.ASK_USER_RESOLVED, { request_id: 'req-1', timed_out: false })

    await waitFor(() => {
      expect(screen.queryByTestId('ask-user-card')).not.toBeInTheDocument()
    })
  })

  it('keeps the answered linger in the window that submitted the answer', async () => {
    await renderCard()
    flush(EVENT_NAMES.ASK_USER_REQUEST, request())
    const group = await screen.findByTestId('ask-user-options')
    fireEvent.click(within(group).getByRole('button', { name: 'Yes' }))
    await waitFor(() => expect(api.respondAskUser).toHaveBeenCalledWith('req-1', ['Yes']))
    expect(await screen.findByTestId('ask-user-status')).toHaveTextContent('Answered')

    // The broadcast triggered by THIS window's own submit arrives back —
    // the settled (answered) card keeps its linger, it is not blanked.
    flush(EVENT_NAMES.ASK_USER_RESOLVED, { request_id: 'req-1', timed_out: false })
    expect(screen.getByTestId('ask-user-status')).toHaveTextContent('Answered')
  })

  it('shows the timed-out cue when the answered broadcast carries timed_out: true', async () => {
    await renderCard()
    flush(EVENT_NAMES.ASK_USER_REQUEST, request())
    await screen.findByTestId('ask-user-card')

    flush(EVENT_NAMES.ASK_USER_RESOLVED, { request_id: 'req-1', timed_out: true })
    expect(await screen.findByTestId('ask-user-status')).toHaveTextContent(/Timed out/i)
  })
})
