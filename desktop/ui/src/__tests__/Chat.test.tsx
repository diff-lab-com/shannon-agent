import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import * as dialog from '@tauri-apps/plugin-dialog'
import { I18nProvider } from '@/i18n'
import Chat from '@/pages/Chat'

const ctx = vi.hoisted(() => ({
  messages: [] as any[],
  streamingText: '',
  thinkingText: '',
  isQuerying: false,
  activeToolCalls: [] as any[],
  usage: null as any,
  sessions: [] as any[],
  currentSessionId: null as string | null,
  error: null as string | null,
  config: null as any,
  status: null as any,
  sendMessage: vi.fn(),
  cancelQuery: vi.fn(),
  checkpoints: [] as unknown[],
  rewindSession: vi.fn(),
  feedback: {} as Record<string, string>,
  recordFeedback: vi.fn().mockResolvedValue(undefined),
  createSession: vi.fn(),
  switchSession: vi.fn(),
  deleteSession: vi.fn(),
  renameSession: vi.fn(),
}))

vi.mock('@/context/ChatContext', () => ({
  useChat: () => ctx,
}))
vi.mock('@/context/SessionContext', () => ({
  useSessions: () => ctx,
}))
vi.mock('@/context/CatalogContext', () => ({
  useCatalog: () => ctx,
}))

function resetCtx() {
  ctx.messages = []
  ctx.streamingText = ''
  ctx.thinkingText = ''
  ctx.isQuerying = false
  ctx.activeToolCalls = []
  ctx.usage = null
  ctx.sessions = []
  ctx.currentSessionId = null
  ctx.error = null
  ctx.config = null
  ctx.status = null
  ctx.sendMessage = vi.fn()
  ctx.cancelQuery = vi.fn()
  ctx.createSession = vi.fn()
  ctx.switchSession = vi.fn()
  ctx.deleteSession = vi.fn()
  ctx.renameSession = vi.fn()
}

function renderChat() {
  return render(
    <I18nProvider>
      <MemoryRouter>
        <Chat />
      </MemoryRouter>
    </I18nProvider>
  )
}

describe('Chat page', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('renders message input area', () => {
    resetCtx()
    renderChat()
    expect(screen.getByPlaceholderText('Ask Shannon anything...')).toBeInTheDocument()
  })

  // U1: the Chat page no longer renders its own session list — the app
  // sidebar's SessionsSection is the single list (see Sidebar.test.tsx).
  it('does not render its own session rail', () => {
    resetCtx()
    renderChat()
    expect(screen.queryByPlaceholderText('Search chats…')).not.toBeInTheDocument()
    expect(screen.queryByText('New Chat')).not.toBeInTheDocument()
  })

  it('sends message on Enter key and clears input', () => {
    resetCtx()
    renderChat()
    const input = screen.getByPlaceholderText('Ask Shannon anything...')
    fireEvent.change(input, { target: { value: 'Hello agent' } })
    fireEvent.keyDown(input, { key: 'Enter' })
    expect(ctx.sendMessage).toHaveBeenCalledWith('Hello agent', undefined)
  })

  it('does not send empty message on Enter', () => {
    resetCtx()
    renderChat()
    const input = screen.getByPlaceholderText('Ask Shannon anything...')
    fireEvent.change(input, { target: { value: '' } })
    fireEvent.keyDown(input, { key: 'Enter' })
    expect(ctx.sendMessage).not.toHaveBeenCalled()
  })

  it('does not send when querying', () => {
    resetCtx()
    ctx.isQuerying = true
    renderChat()
    const input = screen.getByPlaceholderText('Processing...')
    fireEvent.change(input, { target: { value: 'test' } })
    fireEvent.keyDown(input, { key: 'Enter' })
    expect(ctx.sendMessage).not.toHaveBeenCalled()
  })

  it('calls cancelQuery on Escape when querying', () => {
    resetCtx()
    ctx.isQuerying = true
    renderChat()
    const input = screen.getByPlaceholderText('Processing...')
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
    ctx.usage = { input_tokens: 1000, output_tokens: 500, cost_usd: 0.05 }
    renderChat()
    expect(screen.getByText('Usage')).toBeInTheDocument()
    expect(screen.getByText('1,000')).toBeInTheDocument()
    expect(screen.getByText('500')).toBeInTheDocument()
    expect(screen.getByText('$0.0500')).toBeInTheDocument()
  })

  it('renders active tool calls section', () => {
    resetCtx()
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
    expect(screen.getByText(/"cmd"/)).toBeInTheDocument()
    expect(screen.getByText('output here')).toBeInTheDocument()
  })

  it('renders like, copy, and regenerate buttons for assistant messages', () => {
    resetCtx()
    ctx.messages = [{ id: '2', role: 'assistant', content: 'Response' }]
    renderChat()
    expect(screen.getByLabelText('Like message')).toBeInTheDocument()
    expect(screen.getByLabelText('Copy message')).toBeInTheDocument()
    expect(screen.getByLabelText('Regenerate response')).toBeInTheDocument()
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

  // US-CHAT-08: Attach file button — wired to Tauri native dialog.
  it('has attach file button', () => {
    resetCtx()
    renderChat()
    expect(screen.getByLabelText('Attach file')).toBeInTheDocument()
  })

  it('clicking attach button opens Tauri file dialog', async () => {
    resetCtx()
    renderChat()
    fireEvent.click(screen.getByLabelText('Attach file'))
    await waitFor(() => {
      expect(dialog.open).toHaveBeenCalledWith(expect.objectContaining({ multiple: true }))
    })
  })

  it('shows selected file as a chip with basename only', async () => {
    resetCtx()
    vi.mocked(dialog.open).mockResolvedValueOnce('/home/alice/Downloads/report.pdf')
    renderChat()
    fireEvent.click(screen.getByLabelText('Attach file'))
    await waitFor(() => {
      expect(screen.getByText('report.pdf')).toBeInTheDocument()
    })
  })

  it('does not render an HTML file input (uses native dialog instead)', () => {
    resetCtx()
    renderChat()
    expect(document.querySelector('input[type="file"]')).toBeNull()
  })

  // Header working-directory chip was removed when ChatInput took ownership
  // of WD selection. Per-input chip behavior is covered in ChatInput.test.tsx.
  // U1 additionally removed the per-row WD hint + export/print hover buttons
  // (now in the sidebar rail's ⋯ menu — see Sidebar.test.tsx).
  // U2 removed the composer-footer provider/model pill (the global Header is
  // the single model surface) and the per-page ChatHeader bar (title + panel
  // toggle now live in the global Header — see Header.test.tsx).

  describe('API key missing banner', () => {
    it('renders banner when config has no api_key and provider is not ollama', () => {
      resetCtx()
      ctx.config = { provider: 'anthropic' }
      renderChat()
      expect(screen.getByText('Add your API key to start chatting')).toBeInTheDocument()
      expect(screen.getByText('Open Settings')).toBeInTheDocument()
    })

    it('hides banner when api_key is present', () => {
      resetCtx()
      ctx.config = { provider: 'anthropic', api_key: 'sk-xxx' }
      renderChat()
      expect(screen.queryByText('Add your API key to start chatting')).not.toBeInTheDocument()
    })

    it('hides banner when provider is ollama (no key required)', () => {
      resetCtx()
      ctx.config = { provider: 'ollama' }
      renderChat()
      expect(screen.queryByText('Add your API key to start chatting')).not.toBeInTheDocument()
    })

    it('hides banner when user clicks dismiss', () => {
      resetCtx()
      ctx.config = { provider: 'anthropic' }
      renderChat()
      fireEvent.click(screen.getByLabelText('Dismiss'))
      expect(screen.queryByText('Add your API key to start chatting')).not.toBeInTheDocument()
    })

    it('deep-links to /settings/models when CTA clicked', () => {
      resetCtx()
      ctx.config = { provider: 'anthropic' }
      renderChat()
      const cta = screen.getByText('Open Settings').closest('button')!
      expect(cta).toBeInTheDocument()
    })
  })
})
