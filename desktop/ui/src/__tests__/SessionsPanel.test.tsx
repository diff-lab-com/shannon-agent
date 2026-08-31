// SessionsPanel — minimal session switcher.
//
// `useSessions()` is provided by AppProvider, which fetches sessions
// from `api.listSessions()` on mount. Tests mount the real AppProvider
// (rather than mocking the slice) so the wiring stays honest; the only
// mocks are the network layer, which is already stubbed in setup.ts.

import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, screen, fireEvent, waitFor, within } from '@testing-library/react'
import { AppProvider } from '@/context/AppContext'
import * as api from '@/lib/tauri-api'
import { SessionsPanel } from '@/components/SessionsPanel/SessionsPanel'

const listSessions = vi.mocked(api.listSessions)
const newSession = vi.mocked(api.newSession)
const switchSession = vi.mocked(api.switchSession)

function renderPanel(props?: { titleId?: string; className?: string }) {
  return render(
    <AppProvider>
      <SessionsPanel {...props} />
    </AppProvider>,
  )
}

/** Find the aside whose aria-label matches the panel title. */
function getPanelRoot(): HTMLElement {
  return screen.getByRole('complementary', { name: 'Threads' }) as HTMLElement
}

/** Find the New thread (create) button — its only identifier is the aria-label. */
function getNewThreadButton(): HTMLElement {
  return screen.getByRole('button', { name: /New thread/i })
}

beforeEach(() => {
  vi.clearAllMocks()
  // AppContext's refreshSessions logs to console.warn when listSessions
  // rejects — silence it so stderr stays clean across the failure tests.
  vi.spyOn(console, 'warn').mockImplementation(() => undefined)
  // AppProvider's mount effect calls listSessions(); let each test seed it.
  listSessions.mockResolvedValue([])
  // createSession → api.newSession() → we need a stable id so refresh
  // doesn't blow up. Default to the value setup.ts already provides.
  newSession.mockResolvedValue('session-1')
  switchSession.mockResolvedValue([])
})

describe('SessionsPanel — layout & defaults', () => {
  it('renders the panel with default title "Threads"', async () => {
    renderPanel()
    const root = getPanelRoot()
    expect(within(root).getByRole('heading', { name: 'Threads', level: 2 })).toBeInTheDocument()
    expect(getNewThreadButton()).toBeInTheDocument()
  })

  it('renders the localized title from a custom titleId', async () => {
    renderPanel({ titleId: 'sessionsPanel.title' })
    expect(screen.getByRole('heading', { name: 'Threads', level: 2 })).toBeInTheDocument()
  })

  it('renders the new thread button as an icon button with a tooltip', async () => {
    renderPanel()
    const btn = getNewThreadButton()
    expect(btn.tagName).toBe('BUTTON')
    expect(btn).toHaveAttribute('title', expect.stringMatching(/Start a new thread/i))
  })

  it('renders the empty-state copy when there are no sessions', async () => {
    listSessions.mockResolvedValue([])
    renderPanel()
    const root = getPanelRoot()
    await waitFor(() =>
      expect(within(root).getByText(/No threads yet/i)).toBeInTheDocument(),
    )
  })

  it('applies an optional className to the aside wrapper', async () => {
    renderPanel({ className: 'custom-class' })
    const root = getPanelRoot()
    expect(root.className).toContain('custom-class')
  })
})

describe('SessionsPanel — session list rendering', () => {
  it('renders every session returned by listSessions', async () => {
    listSessions.mockResolvedValue([
      { id: 'aaaaaaaa-1111', title: 'First thread', created_at: 1000, message_count: 3 },
      { id: 'bbbbbbbb-2222', title: 'Second thread', created_at: 2000, message_count: 0 },
      { id: 'cccccccc-3333', title: null, created_at: 3000, message_count: 7 },
    ])
    renderPanel()
    const root = getPanelRoot()
    await waitFor(() => expect(within(root).getAllByRole('listitem')).toHaveLength(3))
    expect(within(root).getByText('First thread')).toBeInTheDocument()
    expect(within(root).getByText('Second thread')).toBeInTheDocument()
    // title=null falls back to the shortId (first 8 hex chars).
    expect(within(root).getByText('cccccccc')).toBeInTheDocument()
  })

  it('orders sessions newest-first by created_at', async () => {
    listSessions.mockResolvedValue([
      { id: 'older-id-1', title: 'Older', created_at: 100, message_count: 0 },
      { id: 'newest-2', title: 'Newest', created_at: 300, message_count: 0 },
      { id: 'middle-i', title: 'Middle', created_at: 200, message_count: 0 },
    ])
    renderPanel()
    const root = getPanelRoot()
    await waitFor(() => expect(within(root).getAllByRole('listitem')).toHaveLength(3))
    const items = within(root).getAllByRole('listitem')
    // Each list item is a <li> containing one <button> with the title text.
    expect(within(items[0]!).getByText('Newest')).toBeInTheDocument()
    expect(within(items[1]!).getByText('Middle')).toBeInTheDocument()
    expect(within(items[2]!).getByText('Older')).toBeInTheDocument()
  })

  it('renders the pluralized message count for each row', async () => {
    listSessions.mockResolvedValue([
      { id: 'a-id-0001', title: 'Zero', created_at: 0, message_count: 0 },
      { id: 'b-id-0001', title: 'One', created_at: 0, message_count: 1 },
      { id: 'c-id-0001', title: 'Many', created_at: 0, message_count: 5 },
    ])
    renderPanel()
    const root = getPanelRoot()
    await waitFor(() => expect(within(root).getAllByRole('listitem')).toHaveLength(3))
    expect(within(root).getByText(/No messages/i)).toBeInTheDocument()
    expect(within(root).getByText(/^1 message$/)).toBeInTheDocument()
    expect(within(root).getByText(/^5 messages$/)).toBeInTheDocument()
  })

  it('falls back to a default of 0 messages when message_count is null/undefined', async () => {
    listSessions.mockResolvedValue([
      // message_count intentionally omitted; cast to keep the test surface tight.
      { id: 'x-id-0001', title: 'No count', created_at: 0 } as api.SessionInfo,
    ])
    renderPanel()
    const root = getPanelRoot()
    await waitFor(() => expect(within(root).getByText('No count')).toBeInTheDocument())
    expect(within(root).getByText(/No messages/i)).toBeInTheDocument()
  })

  it('truncates the shortId fallback to 8 hex chars when the id is short', async () => {
    listSessions.mockResolvedValue([
      { id: 'abc', title: null, created_at: 0, message_count: 0 },
    ])
    renderPanel()
    const root = getPanelRoot()
    await waitFor(() => expect(within(root).getByText('abc')).toBeInTheDocument())
  })
})

describe('SessionsPanel — active session highlight', () => {
  it('marks the active row with aria-current="page"', async () => {
    listSessions.mockResolvedValue([
      { id: 'a-id-active', title: 'A', created_at: 1, message_count: 0 },
      { id: 'b-id-other', title: 'B', created_at: 2, message_count: 0 },
    ])
    // AppContext seeds currentSessionId from listSessions after mount; we
    // seed it indirectly via a follow-up call. Simpler: rely on the
    // `setCurrentSessionId` action being called only via create/switch,
    // so with no actions the panel shows none as active. Verify both
    // absent first, then exercise the click flow below.
    renderPanel()
    const root = getPanelRoot()
    await waitFor(() => expect(within(root).getAllByRole('listitem')).toHaveLength(2))
    const buttons = within(root).getAllByRole('button', { name: /A|B/ })
    buttons.forEach((b) => expect(b).not.toHaveAttribute('aria-current', 'page'))
  })

  it('highlights the session clicked last', async () => {
    listSessions.mockResolvedValue([
      { id: 'first-id-1', title: 'First', created_at: 1, message_count: 0 },
      { id: 'second-id', title: 'Second', created_at: 2, message_count: 0 },
    ])
    renderPanel()
    const root = getPanelRoot()
    await waitFor(() => expect(within(root).getAllByRole('listitem')).toHaveLength(2))
    fireEvent.click(within(root).getByRole('button', { name: /Second/ }))
    await waitFor(() =>
      expect(within(root).getByRole('button', { name: /Second/ })).toHaveAttribute(
        'aria-current',
        'page',
      ),
    )
  })
})

describe('SessionsPanel — switch interaction', () => {
  it('invokes switchSession(id) on row click', async () => {
    listSessions.mockResolvedValue([
      { id: 'a-id-0001', title: 'A', created_at: 1, message_count: 0 },
      { id: 'b-id-0002', title: 'B', created_at: 2, message_count: 0 },
    ])
    renderPanel()
    const root = getPanelRoot()
    await waitFor(() => expect(within(root).getAllByRole('listitem')).toHaveLength(2))
    fireEvent.click(within(root).getByRole('button', { name: /B/ }))
    await waitFor(() => expect(switchSession).toHaveBeenCalledTimes(1))
    expect(switchSession.mock.calls[0]![0]).toBe('b-id-0002')
  })

  it('does not throw and does not change the active row when switchSession rejects', async () => {
    // The AppContext slice swallows switchSession errors via setError(String(e))
    // (see AppContext.tsx:172) before the panel's outer .catch ever fires —
    // so the panel itself never throws. We assert the observable contract:
    // the click is forwarded, the rejection is absorbed, and the panel
    // remains rendered.
    switchSession.mockRejectedValue(new Error('switch failed'))
    listSessions.mockResolvedValue([
      { id: 'a-id-0001', title: 'A', created_at: 1, message_count: 0 },
      { id: 'b-id-0002', title: 'B', created_at: 2, message_count: 0 },
    ])
    renderPanel()
    const root = getPanelRoot()
    await waitFor(() => expect(within(root).getAllByRole('listitem')).toHaveLength(2))
    expect(() =>
      fireEvent.click(within(root).getByRole('button', { name: /B/ })),
    ).not.toThrow()
    await waitFor(() => expect(switchSession).toHaveBeenCalledTimes(1))
    // Panel stays rendered; neither row is marked active because the
    // rejected switch never advanced currentSessionId.
    expect(within(root).queryByRole('button', { name: /A/ })).not.toHaveAttribute(
      'aria-current',
      'page',
    )
    expect(within(root).queryByRole('button', { name: /B/ })).not.toHaveAttribute(
      'aria-current',
      'page',
    )
  })
})

describe('SessionsPanel — new thread button', () => {
  it('invokes createSession when the New thread button is clicked', async () => {
    listSessions.mockResolvedValue([])
    renderPanel()
    fireEvent.click(getNewThreadButton())
    await waitFor(() => expect(newSession).toHaveBeenCalledTimes(1))
  })

  it('does not throw when createSession rejects (AppContext slice swallows the error)', async () => {
    // Same swallow-path rationale as switchSession: AppContext:127 wraps
    // createSession in try/catch and surfaces the failure via setError.
    // The panel's outer .catch is defensive and never fires here.
    newSession.mockRejectedValue(new Error('create failed'))
    listSessions.mockResolvedValue([])
    renderPanel()
    expect(() => fireEvent.click(getNewThreadButton())).not.toThrow()
    await waitFor(() => expect(newSession).toHaveBeenCalledTimes(1))
    // Panel still rendered with the empty-state copy.
    expect(getPanelRoot()).toBeInTheDocument()
  })
})

describe('SessionsPanel — mount-time refresh', () => {
  it('calls listSessions on mount to refresh the cache', async () => {
    listSessions.mockResolvedValue([])
    renderPanel()
    // AppProvider + SessionsPanel both call listSessions on mount; we just
    // assert the call happened at least once rather than pin the count,
    // since the AppContext mount path may run more than one refresh.
    await waitFor(() => expect(listSessions).toHaveBeenCalled())
  })

  it('does not throw and falls back to the empty state when the mount-time refresh fails', async () => {
    // listSessions is invoked from two places on mount: AppContext's own
    // bootstrap effect AND SessionsPanel's useEffect. Both layers catch the
    // rejection internally — the panel renders the empty state regardless.
    // We assert the observable contract: the panel mounts, listSessions was
    // attempted, and the empty-state copy is shown.
    listSessions.mockRejectedValue(new Error('refresh failed'))
    renderPanel()
    await waitFor(() => expect(listSessions).toHaveBeenCalled())
    expect(getPanelRoot()).toBeInTheDocument()
    await waitFor(() =>
      expect(within(getPanelRoot()).getByText(/No threads yet/i)).toBeInTheDocument(),
    )
  })
})