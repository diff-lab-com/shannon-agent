import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor, act } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import * as dialog from '@tauri-apps/plugin-dialog'
import { I18nProvider } from '@/i18n'
import { ArtifactProvider } from '@/components/artifact/ArtifactContext'
import Chat from '@/pages/Chat'
import * as api from '@/lib/tauri-api'

const ctx = vi.hoisted(() => ({
  messages: [] as any[],
  streamingText: '',
  thinkingText: '',
  isQuerying: false,
  activeToolCalls: [] as any[],
  // R5-2 — in-stream retry notices (failover / key rotation).
  streamNotices: [] as any[],
  usage: null as any,
  sessions: [] as any[],
  currentSessionId: null as string | null,
  windowSessionId: null as string | null,
  error: null as string | null,
  // 2026-09-29 provider review — the banner/welcome gates read the
  // provider-status snapshot, not the dead `config.provider` fields.
  errorKind: null as 'auth' | 'other' | null,
  providerStatus: null as any,
  config: null as any,
  status: null as any,
  sendMessage: vi.fn().mockResolvedValue(true),
  cancelQuery: vi.fn(),
  checkpoints: [] as unknown[],
  rewindSession: vi.fn(),
  feedback: {} as Record<string, string>,
  recordFeedback: vi.fn().mockResolvedValue(undefined),
  createSession: vi.fn(),
  switchSession: vi.fn(),
  deleteSession: vi.fn(),
  renameSession: vi.fn(),
  // B1 §4-9 — prompt queue surface consumed by Chat + ComposerPanel.
  promptQueue: [] as any[],
  enqueuePrompt: vi.fn().mockReturnValue(true),
  dequeuePrompt: vi.fn().mockReturnValue(null),
  removeQueuedPrompt: vi.fn(),
  moveQueuedPrompt: vi.fn(),
  // P2-19 / GB P2-3 — live while a run streams.
  toolProgress: null as any,
  runProcess: null as any,
  // P0 sidebar telemetry (SessionContext) — read by MessageArea's
  // RunStatusLine while isQuerying.
  sessionActivity: {} as Record<string, any>,
}))

vi.mock('@/context/ChatContext', () => ({
  useChat: () => ctx,
}))
// The real `SessionContext` export stays (ContextBreakdownCard consumes it
// via useContext) — only the hook is overridden.
vi.mock('@/context/SessionContext', async (importOriginal) => {
  const actual = await importOriginal<object>()
  return {
    ...actual,
    useSessions: () => ctx,
  }
})
vi.mock('@/context/CatalogContext', () => ({
  useCatalog: () => ctx,
}))
// R2 W2-4: `api` is imported so the budget-continue tests can drive the
// setup.ts mock defaults (getSessionBudget → null = banners hidden) per
// scenario via vi.mocked(api.getSessionBudget…).

function resetCtx() {
  ctx.messages = []
  ctx.streamingText = ''
  ctx.thinkingText = ''
  ctx.isQuerying = false
  ctx.activeToolCalls = []
  ctx.streamNotices = []
  ctx.usage = null
  ctx.sessions = []
  ctx.currentSessionId = null
  ctx.windowSessionId = null
  ctx.error = null
  ctx.errorKind = null
  ctx.providerStatus = null
  ctx.config = null
  ctx.status = null
  ctx.sendMessage = vi.fn().mockResolvedValue(true)
  ctx.cancelQuery = vi.fn()
  ctx.createSession = vi.fn()
  ctx.switchSession = vi.fn()
  ctx.deleteSession = vi.fn()
  ctx.renameSession = vi.fn()
  ctx.promptQueue = []
  ctx.enqueuePrompt = vi.fn().mockReturnValue(true)
  ctx.dequeuePrompt = vi.fn().mockReturnValue(null)
  ctx.removeQueuedPrompt = vi.fn()
  ctx.moveQueuedPrompt = vi.fn()
  ctx.toolProgress = null
  ctx.runProcess = { status: 'idle', startedAt: null, endedAt: null, sources: [], outputs: [], summary: null, lastTool: null, toolCount: 0 }
  ctx.sessionActivity = {}
  localStorage.clear()
}

// A fresh element per render: rerendering with the SAME element reference
// bails React out (identical props → no re-render → no effect re-run), which
// the steering-integration suite below relies on to settle a run via ctx
// mutation + rerender.
function ChatTree() {
  return (
    <I18nProvider>
      <MemoryRouter>
        {/* Batch D4: the artifact context moved to the app level — mirror
            that nesting here (Chat no longer mounts its own provider). */}
        <ArtifactProvider>
          <Chat />
        </ArtifactProvider>
      </MemoryRouter>
    </I18nProvider>
  )
}

function renderChat() {
  return render(<ChatTree />)
}

describe('Chat page', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('renders message input area', () => {
    resetCtx()
    renderChat()
    expect(screen.getByPlaceholderText(/Try: "Explain this repo"/)).toBeInTheDocument()
  })

  // U1: the Chat page no longer renders its own session list — the app
  // sidebar's SessionsSection is the single list (see Sidebar.test.tsx).
  it('does not render its own session rail', () => {
    resetCtx()
    renderChat()
    expect(screen.queryByPlaceholderText('Search chats…')).not.toBeInTheDocument()
    expect(screen.queryByText('New Chat')).not.toBeInTheDocument()
  })

  // R2 W2-4: the composer clears once the send is ACCEPTED — the send is
  // awaited now, so the clear lands a microtask after Enter.
  it('sends message on Enter key and clears input', async () => {
    resetCtx()
    renderChat()
    const input = screen.getByPlaceholderText(/Try: "Explain this repo"/)
    fireEvent.change(input, { target: { value: 'Hello agent' } })
    fireEvent.keyDown(input, { key: 'Enter' })
    expect(ctx.sendMessage).toHaveBeenCalledWith('Hello agent', undefined)
    await waitFor(() => expect(input).toHaveValue(''))
  })

  it('does not send empty message on Enter', () => {
    resetCtx()
    renderChat()
    const input = screen.getByPlaceholderText(/Try: "Explain this repo"/)
    fireEvent.change(input, { target: { value: '' } })
    fireEvent.keyDown(input, { key: 'Enter' })
    expect(ctx.sendMessage).not.toHaveBeenCalled()
  })

  // B0 P0-1 — attachments-only sends are real: the backend's send_message
  // accepts an empty text alongside attachment paths (the engine turns the
  // attachments into content blocks), so Enter/Submit with files and no
  // text goes through instead of silently no-oping.
  it('sends attachments-only (empty text) instead of silently no-oping', async () => {
    resetCtx()
    ctx.currentSessionId = 'sess-1'
    // No working_dir on the session — keeps the default composer placeholder
    // this file's queries match against.
    ctx.sessions = [{ id: 'sess-1', title: 'S' }]
    vi.mocked(dialog.open).mockResolvedValueOnce('/home/alice/Downloads/report.pdf')
    renderChat()
    fireEvent.click(screen.getByLabelText('Attachments and tools'))
    fireEvent.click(screen.getByRole('menuitem', { name: 'Attach file' }))
    await screen.findByText('report.pdf')

    const input = screen.getByPlaceholderText(/Try: "Explain this repo"/)
    fireEvent.keyDown(input, { key: 'Enter' })
    expect(ctx.sendMessage).toHaveBeenCalledWith('', ['/home/alice/Downloads/report.pdf'])
  })

  // B1 §4-9 — a send while THIS session streams joins the FIFO queue
  // instead of being dropped (sendMessage must stay untouched).
  it('queues the message instead of sending while querying', () => {
    resetCtx()
    ctx.isQuerying = true
    renderChat()
    const input = screen.getByPlaceholderText(/Reply generating — press Enter to queue/)
    fireEvent.change(input, { target: { value: 'queued hello' } })
    fireEvent.keyDown(input, { key: 'Enter' })
    expect(ctx.sendMessage).not.toHaveBeenCalled()
    expect(ctx.enqueuePrompt).toHaveBeenCalledWith('queued hello', [])
    // an accepted enqueue clears the draft
    expect(input).toHaveValue('')
  })

  it('keeps the draft when the queue is full (enqueue rejected)', () => {
    resetCtx()
    ctx.isQuerying = true
    ctx.enqueuePrompt = vi.fn().mockReturnValue(false)
    renderChat()
    const input = screen.getByPlaceholderText(/Reply generating — press Enter to queue/)
    fireEvent.change(input, { target: { value: 'never queued' } })
    fireEvent.keyDown(input, { key: 'Enter' })
    expect(ctx.enqueuePrompt).toHaveBeenCalled()
    expect(input).toHaveValue('never queued')
  })

  it('calls cancelQuery on Escape when querying', () => {
    resetCtx()
    ctx.isQuerying = true
    renderChat()
    const input = screen.getByPlaceholderText(/Reply generating — press Enter to queue/)
    fireEvent.keyDown(input, { key: 'Escape' })
    expect(ctx.cancelQuery).toHaveBeenCalled()
  })

  it('renders user message bubble', () => {
    resetCtx()
    ctx.messages = [{ id: '1', role: 'user', content: 'Hello there' }]
    renderChat()
    expect(screen.getByText('Hello there')).toBeInTheDocument()
  })

  it('renders assistant message bubble', () => {
    resetCtx()
    ctx.messages = [{ id: '2', role: 'assistant', content: 'Hi from assistant' }]
    renderChat()
    expect(screen.getByText('Hi from assistant')).toBeInTheDocument()
  })

  it('renders streaming text when present', () => {
    resetCtx()
    ctx.streamingText = 'Streaming response...'
    renderChat()
    expect(screen.getByText('Streaming response...')).toBeInTheDocument()
  })

  it('renders thinking text inside the collapsible Reasoning block when present', () => {
    resetCtx()
    ctx.thinkingText = 'Thinking about this...'
    renderChat()
    // P2-5d — thinking now lives inside a collapsible "Reasoning"
    // block (aria-expanded toggle). The button is always rendered.
    const toggle = screen.getByRole('button', { name: /thinking/i })
    expect(toggle).toBeInTheDocument()
    expect(toggle).toHaveAttribute('aria-expanded')
  })

  it('renders usage section when usage data present', () => {
    resetCtx()
    // The usage cards live in the right dock's Context tab (P1-⑦).
    ctx.contextPanelOpen = true
    ctx.usage = { input_tokens: 1000, output_tokens: 500, cost_usd: 0.05 }
    renderChat()
    expect(screen.getByText('Usage')).toBeInTheDocument()
    expect(screen.getByText('1,000')).toBeInTheDocument()
    expect(screen.getByText('500')).toBeInTheDocument()
    expect(screen.getByText('$0.0500')).toBeInTheDocument()
  })

  it('renders active tool calls section', () => {
    resetCtx()
    ctx.contextPanelOpen = true
    ctx.activeToolCalls = [{ tool_use_id: 'tc1', tool_name: 'bash', status: 'running' }]
    renderChat()
    expect(screen.getByText('Active Tools')).toBeInTheDocument()
    expect(screen.getAllByText('bash').length).toBeGreaterThan(0)
  })

  it('renders tool call with error status', () => {
    resetCtx()
    ctx.activeToolCalls = [{ tool_use_id: 'tc1', tool_name: 'read_file', status: 'error' }]
    renderChat()
    expect(screen.getAllByText('read_file').length).toBeGreaterThan(0)
  })

  it('renders tool call with completed status', () => {
    resetCtx()
    ctx.activeToolCalls = [{ tool_use_id: 'tc1', tool_name: 'write_file', status: 'completed' }]
    renderChat()
    expect(screen.getAllByText('write_file').length).toBeGreaterThan(0)
  })

  it('renders error message when error present', () => {
    resetCtx()
    ctx.error = 'Something went wrong'
    renderChat()
    expect(screen.getByText(/Something went wrong/)).toBeInTheDocument()
  })

  // B0 P1-3 — the error banner's Retry resends the LAST USER MESSAGE (the
  // composer was cleared on send, so the old text-gated retry never fired).
  it('retry resends the last user message', () => {
    resetCtx()
    ctx.error = 'engine exploded'
    ctx.messages = [
      { id: '1', role: 'user', content: 'first question', timestamp: 1 },
      { id: '2', role: 'assistant', content: 'answer', timestamp: 2 },
      { id: '3', role: 'user', content: 'failing question', timestamp: 3 },
    ]
    renderChat()
    fireEvent.click(screen.getByText('Retry'))
    expect(ctx.sendMessage).toHaveBeenCalledTimes(1)
    expect(ctx.sendMessage).toHaveBeenCalledWith('failing question')
  })

  it('retry is hidden when there is no previous user message to resend', () => {
    resetCtx()
    ctx.error = 'engine exploded'
    ctx.messages = []
    renderChat()
    expect(screen.queryByText('Retry')).not.toBeInTheDocument()
  })

  // B0 P2-2 — a slash result card is session-scoped: switching sessions
  // clears it instead of letting /cost follow the user into the next one.
  it('clears the slash result card when the session changes', async () => {
    resetCtx()
    ctx.currentSessionId = 'sess-a'
    ctx.sessions = [{ id: 'sess-a', title: 'A' }]
    const view = renderChat()
    const input = screen.getByPlaceholderText(/Try: "Explain this repo"/)
    fireEvent.change(input, { target: { value: '/cost' } })
    fireEvent.keyDown(input, { key: 'Enter' })
    // /cost runs and pins its card (cost line from the mocked usage) above
    // the composer.
    expect(await screen.findByText('$0.01')).toBeInTheDocument()

    ctx.currentSessionId = 'sess-b'
    await act(async () => {
      view.rerender(
        <ChatTree />,
      )
    })
    expect(screen.queryByText('$0.01')).not.toBeInTheDocument()
  })

  it('renders assistant message with tool calls', () => {
    resetCtx()
    ctx.messages = [{
      id: '3', role: 'assistant', content: 'Let me check that.',
      tool_calls: [{ tool_use_id: 'tc1', tool_name: 'read_file', status: 'completed', tool_input: { path: '/test' }, result: 'file contents' }],
    }]
    renderChat()
    expect(screen.getByText('Let me check that.')).toBeInTheDocument()
    expect(screen.getByText('read_file')).toBeInTheDocument()
  })

  it('expands tool call on click', () => {
    resetCtx()
    ctx.messages = [{
      id: '3', role: 'assistant', content: 'Checking.',
      tool_calls: [{ tool_use_id: 'tc1', tool_name: 'bash', status: 'completed', tool_input: { cmd: 'ls' }, result: 'output here' }],
    }]
    renderChat()
    fireEvent.click(screen.getByText('bash'))
    // 2026-09 P1-3: tool input renders a human summary (label: value) instead
    // of the raw JSON dump — short commands read as one line and stay scannable.
    expect(screen.getByText('ls')).toBeInTheDocument()
    expect(screen.getByText('output here')).toBeInTheDocument()
  })

  it('renders like and copy buttons for assistant messages', () => {
    resetCtx()
    ctx.messages = [{ id: '2', role: 'assistant', content: 'Response' }]
    renderChat()
    expect(screen.getByLabelText('Like message')).toBeInTheDocument()
    expect(screen.getByLabelText('Copy message')).toBeInTheDocument()
  })

  // B1 §4-7 — true regenerate renders ONLY on the last assistant message and
  // only when a checkpoint covers the preceding user turn.
  it('shows the regenerate button on the last assistant message when rewindable', () => {
    resetCtx()
    ctx.messages = [
      { id: '1', role: 'user', content: 'Question one' },
      { id: '2', role: 'assistant', content: 'Answer one' },
      { id: '3', role: 'user', content: 'Question two' },
      { id: '4', role: 'assistant', content: 'Answer two' },
    ]
    ctx.checkpoints = [{ turn_index: 0 }, { turn_index: 1 }]
    renderChat()
    const regen = screen.getAllByLabelText('Regenerate response')
    expect(regen).toHaveLength(1)
  })

  it('hides the regenerate button when no checkpoint covers the turn', () => {
    resetCtx()
    ctx.messages = [
      { id: '1', role: 'user', content: 'Question one' },
      { id: '2', role: 'assistant', content: 'Answer one' },
    ]
    ctx.checkpoints = []
    renderChat()
    expect(screen.queryByLabelText('Regenerate response')).not.toBeInTheDocument()
  })

  it('toggles like state on click', () => {
    resetCtx()
    ctx.messages = [{ id: '2', role: 'assistant', content: 'Response' }]
    renderChat()
    const likeBtn = screen.getByLabelText('Like message')
    fireEvent.click(likeBtn)
    // After liking, the icon changes to thumb_up
    expect(likeBtn.querySelector('.material-symbols-outlined')).toHaveTextContent('thumb_up')
  })

  // US-CHAT-08: Attach file — wired to Tauri native dialog, behind the
  // composer "+" menu (2026-09 review).
  it('has attach entry in the composer "+" menu', () => {
    resetCtx()
    renderChat()
    fireEvent.click(screen.getByLabelText('Attachments and tools'))
    expect(screen.getByRole('menuitem', { name: 'Attach file' })).toBeInTheDocument()
  })

  it('clicking attach menu item opens Tauri file dialog', async () => {
    resetCtx()
    renderChat()
    fireEvent.click(screen.getByLabelText('Attachments and tools'))
    fireEvent.click(screen.getByRole('menuitem', { name: 'Attach file' }))
    await waitFor(() => {
      expect(dialog.open).toHaveBeenCalledWith(expect.objectContaining({ multiple: true }))
    })
  })

  it('shows selected file as a chip with basename only', async () => {
    resetCtx()
    vi.mocked(dialog.open).mockResolvedValueOnce('/home/alice/Downloads/report.pdf')
    renderChat()
    fireEvent.click(screen.getByLabelText('Attachments and tools'))
    fireEvent.click(screen.getByRole('menuitem', { name: 'Attach file' }))
    await waitFor(() => {
      expect(screen.getByText('report.pdf')).toBeInTheDocument()
    })
  })

  it('does not render an HTML file input (uses native dialog instead)', () => {
    resetCtx()
    renderChat()
    expect(document.querySelector('input[type="file"]')).toBeNull()
  })

  // ── US4 direction A: terminal selection → composer prefill ─────────────
  it('prefills the composer and refocuses it on shannon:composer-prefill', () => {
    resetCtx()
    renderChat()
    const input = screen.getByPlaceholderText(/Try: "Explain this repo"/)
    act(() => {
      window.dispatchEvent(new CustomEvent('shannon:composer-prefill', {
        detail: { text: '```\nnpm ERR! missing script\n```' },
      }))
    })
    // The quoted block becomes the draft…
    expect(input).toHaveValue('```\nnpm ERR! missing script\n```')
    // …and the composer is focused so typing continues below it.
    expect(input).toHaveFocus()
  })

  it('ignores shannon:composer-prefill payloads without text', () => {
    resetCtx()
    renderChat()
    const input = screen.getByPlaceholderText(/Try: "Explain this repo"/)
    fireEvent.change(input, { target: { value: 'kept draft' } })
    act(() => {
      window.dispatchEvent(new CustomEvent('shannon:composer-prefill', { detail: {} }))
      window.dispatchEvent(new CustomEvent('shannon:composer-prefill', { detail: { text: '' } }))
      window.dispatchEvent(new CustomEvent('shannon:composer-prefill', { detail: { text: 42 } }))
    })
    expect(input).toHaveValue('kept draft')
  })

  // Header working-directory chip was removed when ChatInput took ownership
  // of WD selection. Per-input chip behavior is covered in ChatInput.test.tsx.
  // U1 additionally removed the per-row WD hint + export/print hover buttons
  // (now in the sidebar rail's ⋯ menu — see Sidebar.test.tsx).
  // U2 removed the composer-footer provider/model pill (the global Header is
  // the single model surface) and the per-page ChatHeader bar (title + panel
  // toggle now live in the global Header — see Header.test.tsx).

  // ── B1 §4-9: queue drain ────────────────────────────────────────────────
  it('auto-sends the queued head when the session stops querying (drain)', () => {
    resetCtx()
    const head = { id: 7, text: 'queued follow-up', attachments: ['/tmp/a.png'] }
    ctx.promptQueue = [head]
    ctx.dequeuePrompt = vi.fn().mockReturnValue(head)
    renderChat()
    expect(ctx.dequeuePrompt).toHaveBeenCalled()
    expect(ctx.sendMessage).toHaveBeenCalledWith('queued follow-up', ['/tmp/a.png'])
  })

  it('does not drain while still querying or with an empty queue', () => {
    resetCtx()
    ctx.isQuerying = true
    ctx.promptQueue = [{ id: 7, text: 'queued', attachments: [] }]
    renderChat()
    expect(ctx.dequeuePrompt).not.toHaveBeenCalled()

    resetCtx()
    ctx.isQuerying = false
    ctx.promptQueue = []
    renderChat()
    expect(ctx.dequeuePrompt).not.toHaveBeenCalled()
  })

  it('renders queued prompts as removable chips', () => {
    resetCtx()
    ctx.promptQueue = [{ id: 7, text: 'queued follow-up', attachments: [] }]
    renderChat()
    expect(screen.getByTestId('prompt-queue')).toBeInTheDocument()
    expect(screen.getByText('queued follow-up')).toBeInTheDocument()
    const remove = screen.getByLabelText('Remove queued message')
    fireEvent.click(remove)
    expect(ctx.removeQueuedPrompt).toHaveBeenCalledWith(7)
  })

  // ── B1 §4-8: message edit ───────────────────────────────────────────────
  const editHistory = [
    { id: 'u1', role: 'user', content: 'original question', timestamp: Date.UTC(2026, 0, 1, 10, 0) },
    { id: 'a1', role: 'assistant', content: 'first answer', timestamp: Date.UTC(2026, 0, 1, 10, 1) },
  ]

  it('edit flow: composer prefills with the message and sending rewinds + resends', async () => {
    resetCtx()
    ctx.messages = editHistory
    ctx.checkpoints = [{ turn_index: 0 }]
    ctx.rewindSession = vi.fn().mockResolvedValue(undefined)
    renderChat()
    // seed a draft, then start editing
    const input = screen.getByPlaceholderText(/Try: "Explain this repo"/)
    fireEvent.change(input, { target: { value: 'my precious draft' } })
    fireEvent.click(screen.getByLabelText('Edit message'))

    // composer is prefilled with the message text; banner names the target
    expect(input).toHaveValue('original question')
    expect(screen.getByTestId('edit-banner')).toBeInTheDocument()

    // edited send → rewind to before the turn, then resend the new text
    fireEvent.change(input, { target: { value: 'edited question' } })
    fireEvent.keyDown(input, { key: 'Enter' })
    await waitFor(() => expect(ctx.rewindSession).toHaveBeenCalledWith(0))
    await waitFor(() => expect(ctx.sendMessage).toHaveBeenCalledWith('edited question', undefined))
    // editing state is left once the send is underway
    expect(screen.queryByTestId('edit-banner')).not.toBeInTheDocument()
  })

  it('edit flow: cancel (banner ✕) restores the pre-edit draft', () => {
    resetCtx()
    ctx.messages = editHistory
    ctx.checkpoints = [{ turn_index: 0 }]
    renderChat()
    const input = screen.getByPlaceholderText(/Try: "Explain this repo"/)
    fireEvent.change(input, { target: { value: 'my precious draft' } })
    fireEvent.click(screen.getByLabelText('Edit message'))
    expect(input).toHaveValue('original question')

    fireEvent.click(screen.getByLabelText('Cancel editing and restore draft'))
    expect(screen.queryByTestId('edit-banner')).not.toBeInTheDocument()
    expect(input).toHaveValue('my precious draft')
  })

  it('edit flow: Escape in the composer cancels the edit', () => {
    resetCtx()
    ctx.messages = editHistory
    ctx.checkpoints = [{ turn_index: 0 }]
    renderChat()
    const input = screen.getByPlaceholderText(/Try: "Explain this repo"/)
    fireEvent.click(screen.getByLabelText('Edit message'))
    fireEvent.keyDown(input, { key: 'Escape' })
    expect(screen.queryByTestId('edit-banner')).not.toBeInTheDocument()
    expect(input).toHaveValue('')
  })

  it('edit button is hidden without a covering checkpoint and disabled while querying', () => {
    resetCtx()
    ctx.messages = editHistory
    ctx.checkpoints = []
    renderChat()
    expect(screen.queryByLabelText('Edit message')).not.toBeInTheDocument()

    resetCtx()
    ctx.messages = editHistory
    ctx.checkpoints = [{ turn_index: 0 }]
    ctx.isQuerying = true
    renderChat()
    expect(screen.getByLabelText('Edit message')).toBeDisabled()
  })

  // ── B1 §4-11: per-session drafts ────────────────────────────────────────
  it('persists the draft per session (debounced) and restores it on switch', () => {
    vi.useFakeTimers()
    try {
      resetCtx()
      ctx.currentSessionId = 'sess-1'
      const view = renderChat()
      const input = screen.getByPlaceholderText(/Try: "Explain this repo"/)
      fireEvent.change(input, { target: { value: 'sess-1 draft' } })
      act(() => { vi.advanceTimersByTime(300) })
      const saved = JSON.parse(localStorage.getItem('shannon.draft.sess-1')!)
      expect(saved.text).toBe('sess-1 draft')

      // switch to sess-2: composer replaced by sess-2's (absent) draft
      ctx.currentSessionId = 'sess-2'
      act(() => {
        view.rerender(
        <ChatTree />,
        )
      })
      expect(input).toHaveValue('')

      // ...and back: sess-1's draft is restored (its flush on leave kept it)
      ctx.currentSessionId = 'sess-1'
      act(() => {
        view.rerender(
        <ChatTree />,
        )
      })
      expect(input).toHaveValue('sess-1 draft')
    } finally {
      vi.useRealTimers()
    }
  })

  it('clears the draft key on send', async () => {
    vi.useFakeTimers()
    try {
      resetCtx()
      ctx.currentSessionId = 'sess-1'
      renderChat()
      const input = screen.getByPlaceholderText(/Try: "Explain this repo"/)
      fireEvent.change(input, { target: { value: 'to be sent' } })
      act(() => { vi.advanceTimersByTime(300) })
      expect(localStorage.getItem('shannon.draft.sess-1')).not.toBeNull()

      fireEvent.keyDown(input, { key: 'Enter' })
      expect(ctx.sendMessage).toHaveBeenCalledWith('to be sent', undefined)
      // R2 W2-4: the draft is cleared once the send is accepted — flush the
      // confirmation microtask (real-timer waits would fight the fake ones).
      await act(async () => {})
      expect(localStorage.getItem('shannon.draft.sess-1')).toBeNull()
    } finally {
      vi.useRealTimers()
    }
  })

  // ── B1 P2-3: session-switch skeleton ────────────────────────────────────
  it('shows the switch overlay only while a session swap is in flight', () => {
    resetCtx()
    renderChat()
    expect(screen.queryByTestId('session-switch-overlay')).not.toBeInTheDocument()

    resetCtx()
    ctx.switchingSession = true
    renderChat()
    expect(screen.getByTestId('session-switch-overlay')).toBeInTheDocument()
  })

  // 2026-09-29 provider review §3-A1: the banner gates on the
  // get_provider_status snapshot (`config.api_key`/`config.provider` are
  // dead since ADR-0005). Each test drives the snapshot directly.
  describe('API key missing banner', () => {
    it('renders banner when nothing is configured (no active, no env provider)', () => {
      resetCtx()
      ctx.providerStatus = {
        active_provider_id: null, display_name: null, kind: null,
        has_api_key: false, model: null, env_provider: null,
      }
      renderChat()
      expect(screen.getByText('Add your API key to start chatting')).toBeInTheDocument()
      expect(screen.getByText('Open Settings')).toBeInTheDocument()
    })

    it('renders the named-provider variant when the active provider has no key', () => {
      resetCtx()
      ctx.providerStatus = {
        active_provider_id: 'anthropic-main', display_name: 'Anthropic', kind: 'anthropic',
        has_api_key: false, model: null, env_provider: null,
      }
      renderChat()
      expect(screen.getByText('API key missing for Anthropic')).toBeInTheDocument()
    })

    it('hides banner when the active provider has a key (configured + keyed users)', () => {
      resetCtx()
      ctx.providerStatus = {
        active_provider_id: 'anthropic-main', display_name: 'Anthropic', kind: 'anthropic',
        has_api_key: true, model: null, env_provider: null,
      }
      renderChat()
      expect(screen.queryByText('Add your API key to start chatting')).not.toBeInTheDocument()
      expect(screen.queryByText(/API key missing for/)).not.toBeInTheDocument()
    })

    it('hides banner when an env provider is detected without a stored one', () => {
      resetCtx()
      ctx.providerStatus = {
        active_provider_id: null, display_name: null, kind: null,
        has_api_key: false, model: null, env_provider: 'anthropic',
      }
      renderChat()
      expect(screen.queryByText('Add your API key to start chatting')).not.toBeInTheDocument()
    })

    it('hides banner when the active provider is ollama (no key required)', () => {
      resetCtx()
      ctx.providerStatus = {
        active_provider_id: 'ollama-local', display_name: 'Ollama', kind: 'ollama',
        has_api_key: false, model: null, env_provider: null,
      }
      renderChat()
      expect(screen.queryByText('Add your API key to start chatting')).not.toBeInTheDocument()
    })

    it('hides banner while the snapshot is unavailable (null)', () => {
      resetCtx()
      ctx.providerStatus = null
      renderChat()
      expect(screen.queryByText('Add your API key to start chatting')).not.toBeInTheDocument()
    })

    it('hides banner when user clicks dismiss', () => {
      resetCtx()
      ctx.providerStatus = {
        active_provider_id: null, display_name: null, kind: null,
        has_api_key: false, model: null, env_provider: null,
      }
      renderChat()
      fireEvent.click(screen.getByLabelText('Dismiss'))
      expect(screen.queryByText('Add your API key to start chatting')).not.toBeInTheDocument()
    })

    it('deep-links to /settings/models when CTA clicked', () => {
      resetCtx()
      ctx.providerStatus = {
        active_provider_id: null, display_name: null, kind: null,
        has_api_key: false, model: null, env_provider: null,
      }
      renderChat()
      const cta = screen.getByText('Open Settings').closest('button')!
      expect(cta).toBeInTheDocument()
    })
  })

  // Review §2-3: query failures classified Rust-side as error_kind="auth"
  // get the dedicated update-key banner; others keep the raw error line.
  describe('auth failure banner', () => {
    const baseStatus = {
      active_provider_id: 'anthropic-main', display_name: 'Anthropic', kind: 'anthropic',
      has_api_key: true, model: null, env_provider: null,
    }

    it('shows the update-key banner with the provider name for auth errors', () => {
      resetCtx()
      ctx.error = 'Authentication failed for anthropic'
      ctx.errorKind = 'auth'
      ctx.providerStatus = baseStatus
      renderChat()
      expect(screen.getByTestId('auth-error-banner')).toBeInTheDocument()
      expect(screen.getByText('API key rejected by Anthropic')).toBeInTheDocument()
      expect(screen.getByText('Update key')).toBeInTheDocument()
    })

    it('keeps the raw error line for non-auth errors', () => {
      resetCtx()
      ctx.error = 'Network unreachable'
      ctx.errorKind = 'other'
      ctx.providerStatus = baseStatus
      renderChat()
      expect(screen.queryByTestId('auth-error-banner')).not.toBeInTheDocument()
      expect(screen.getByText('Network unreachable')).toBeInTheDocument()
    })
  })


  // ── GB P2-10a round-1 review (Imp-1/Imp-2) — steering ↔ drain integration ──
  //
  // Full-page harness: the mocked contexts feed real Chat wiring (composer →
  // handleSteer → useSteerSend, effects → queue drain), so these pin the
  // exact race the review flagged.
  describe('steering integration (round-1)', () => {
    function renderChatRerenderable() {
      const view = render(<ChatTree />)
      return { view, settle: () => view.rerender(<ChatTree />) }
    }

    it('Imp-1: queued prompts are NOT burned when a steer settles — the drain stays gated until the delivery resolves', async () => {
      resetCtx()
      ctx.currentSessionId = 'sess-1'
      ctx.isQuerying = true
      // Two prompts already queued behind the running turn.
      ctx.promptQueue = [
        { id: 1, text: 'queued one', attachments: [] },
        { id: 2, text: 'queued two', attachments: [] },
      ]
      let resolveDelivery!: (ok: boolean) => void
      ctx.sendMessage = vi.fn(() => new Promise<boolean>(res => { resolveDelivery = res }))
      const { settle } = renderChatRerenderable()

      // The user interrupts while the turn streams (Ctrl+Enter).
      const input = screen.getByPlaceholderText(/Reply generating/)
      fireEvent.change(input, { target: { value: 'interrupt: use Rust' } })
      fireEvent.keyDown(input, { key: 'Enter', ctrlKey: true })
      expect(ctx.cancelQuery).toHaveBeenCalledTimes(1)
      expect(ctx.sendMessage).not.toHaveBeenCalled()
      expect(ctx.dequeuePrompt).not.toHaveBeenCalled()

      // The interrupted run settles (QUERY_CANCELLED → isQuerying false).
      ctx.isQuerying = false
      act(() => { settle() })

      // The STEER text goes out; the queue head stays queued — the drain
      // effect ran in this very commit but was gated by hasPendingSteer.
      await waitFor(() => expect(ctx.sendMessage).toHaveBeenCalledWith('interrupt: use Rust', undefined))
      expect(ctx.sendMessage).toHaveBeenCalledTimes(1)
      expect(ctx.dequeuePrompt).not.toHaveBeenCalled()
      expect(ctx.promptQueue).toHaveLength(2)

      // Once the delivery resolves, the gate drops — the queue drains on the
      // NEXT settle, not inside this one.
      await act(async () => { resolveDelivery(true) })
      expect(ctx.dequeuePrompt).not.toHaveBeenCalled()
    })

    it('Imp-2: a steer parked for session A is never delivered to session B; returning to A delivers it', async () => {
      resetCtx()
      ctx.currentSessionId = 'sess-A'
      ctx.isQuerying = true
      ctx.promptQueue = [{ id: 9, text: 'queued for B', attachments: [] }]
      const { settle } = renderChatRerenderable()

      const input = screen.getByPlaceholderText(/Reply generating/)
      fireEvent.change(input, { target: { value: 'belongs to A' } })
      fireEvent.keyDown(input, { key: 'Enter', ctrlKey: true })
      expect(ctx.cancelQuery).toHaveBeenCalledTimes(1)

      // The user switches to session B (which has its OWN, empty queue) and
      // the run settles while B is up.
      ctx.currentSessionId = 'sess-B'
      ctx.promptQueue = []
      ctx.isQuerying = false
      act(() => { settle() })
      await act(async () => { await Promise.resolve() })
      // A's text must not land in B — and B's drain must not be gated by
      // A's parked steer (B has nothing queued: no dequeue attempt either).
      expect(ctx.sendMessage).not.toHaveBeenCalled()
      expect(ctx.dequeuePrompt).not.toHaveBeenCalled()

      // Back on A: the parked steer delivers to A.
      ctx.currentSessionId = 'sess-A'
      act(() => { settle() })
      await waitFor(() => expect(ctx.sendMessage).toHaveBeenCalledWith('belongs to A', undefined))
    })
  })

  // ── R2 W2-4: budget block → return-to-composer + honest "Continue once" ──
  //
  // The pre-turn budget guard rejects a send AFTER the optimistic append
  // (AppContext rolls that back); the page used to clear the composer
  // unconditionally AND "Continue once" reverse-found the last remaining
  // user message — an EARLIER turn — so "continue" replayed an old question
  // and the blocked draft (with its attachments) vanished. These pin the
  // contract: the blocked payload goes back to the composer, the banner's
  // Continue re-sends exactly that payload, and a first-turn block (nothing
  // recorded yet) still has a working — or absent, never dead — action.
  describe('budget continue (R2 W2-4)', () => {
    const CONTINUE_BLOCKED = 'Continue — send the blocked message (ignore once)'
    const CONTINUE_LAST = 'Continue — resend the last message (ignore once)'

    beforeEach(() => {
      // Defaults: no cap → banners hidden (the setup.ts mock baseline).
      // Over-cap scenarios opt in via overCapSession().
      vi.mocked(api.getSessionBudget).mockResolvedValue(null)
      vi.mocked(api.getSessionUsage).mockResolvedValue({
        input_tokens: 0, output_tokens: 0, cache_creation_tokens: 0,
        cache_read_tokens: 0, cost_usd: 0, events: 0,
      } as any)
    })

    function overCapSession() {
      ctx.currentSessionId = 'sess-1'
      ctx.sessions = [{ id: 'sess-1', title: 'S' }]
      // spent >= cap → the exceeded banner re-derives on mount (B4 P2-8).
      vi.mocked(api.getSessionBudget).mockResolvedValue(5)
      vi.mocked(api.getSessionUsage).mockResolvedValue({ cost_usd: 5.2 } as any)
    }

    // 失真① — a blocked send is returned to the composer, attachments included.
    it('returns the blocked draft — text and attachment chip — to the composer on rejection', async () => {
      resetCtx()
      overCapSession()
      ctx.sendMessage = vi.fn().mockResolvedValue(false)
      renderChat()
      vi.mocked(dialog.open).mockResolvedValueOnce('/home/alice/Downloads/report.pdf')
      fireEvent.click(screen.getByLabelText('Attachments and tools'))
      fireEvent.click(screen.getByRole('menuitem', { name: 'Attach file' }))
      await screen.findByText('report.pdf')

      const input = screen.getByPlaceholderText(/Try: "Explain this repo"/)
      fireEvent.change(input, { target: { value: 'blocked question' } })
      fireEvent.keyDown(input, { key: 'Enter' })
      expect(ctx.sendMessage).toHaveBeenCalledWith('blocked question', ['/home/alice/Downloads/report.pdf'])
      // The whole draft survives the rejection: text AND the chip.
      await waitFor(() => expect(input).toHaveValue('blocked question'))
      expect(screen.getByText('report.pdf')).toBeInTheDocument()
    })

    // 失真② — Continue once delivers the blocked payload, never an earlier turn.
    it('Continue once re-sends the blocked payload, not an earlier recorded turn', async () => {
      resetCtx()
      overCapSession()
      ctx.messages = [{ id: '1', role: 'user', content: 'older question', timestamp: 1 }]
      ctx.sendMessage = vi.fn()
        .mockResolvedValueOnce(false) // the pre-turn guard refuses this one
        .mockResolvedValue(true)      // the bypassed continue goes through
      renderChat()
      await screen.findByText('Session budget exceeded')

      const input = screen.getByPlaceholderText(/Try: "Explain this repo"/)
      fireEvent.change(input, { target: { value: 'fresh blocked question' } })
      fireEvent.keyDown(input, { key: 'Enter' })
      await waitFor(() => expect(ctx.sendMessage).toHaveBeenCalledWith('fresh blocked question', undefined))

      // The banner now holds the blocked payload — its label says so.
      fireEvent.click(await screen.findByRole('button', { name: CONTINUE_BLOCKED }))
      await waitFor(() =>
        expect(ctx.sendMessage).toHaveBeenLastCalledWith('fresh blocked question', undefined, { budgetBypass: true }))
      // The old reverse-find would have replayed the earlier turn instead.
      expect(ctx.sendMessage).not.toHaveBeenCalledWith('older question', undefined)
      expect(ctx.sendMessage).not.toHaveBeenCalledWith('older question')
    })

    // 失真③ — a first-turn block has a working action, and with nothing to
    // deliver the action hides (no clickable no-op).
    it('first-turn block keeps Continue alive; with no payload and no recorded turn it hides', async () => {
      resetCtx()
      overCapSession()
      ctx.messages = [] // first turn — nothing recorded yet
      ctx.sendMessage = vi.fn()
        .mockResolvedValueOnce(false)
        .mockResolvedValue(true)
      renderChat()
      await screen.findByText('Session budget exceeded')
      // Nothing refused yet and no user turn recorded → the dead button of
      // the old UI is now simply not there.
      expect(screen.queryByRole('button', { name: CONTINUE_BLOCKED })).not.toBeInTheDocument()
      expect(screen.queryByRole('button', { name: CONTINUE_LAST })).not.toBeInTheDocument()

      const input = screen.getByPlaceholderText(/Try: "Explain this repo"/)
      fireEvent.change(input, { target: { value: 'first ever message' } })
      fireEvent.keyDown(input, { key: 'Enter' })
      await waitFor(() => expect(ctx.sendMessage).toHaveBeenCalledTimes(1))

      // The blocked first message becomes the continue target — alive again.
      fireEvent.click(await screen.findByRole('button', { name: CONTINUE_BLOCKED }))
      await waitFor(() =>
        expect(ctx.sendMessage).toHaveBeenLastCalledWith('first ever message', undefined, { budgetBypass: true }))
    })
  })
})
