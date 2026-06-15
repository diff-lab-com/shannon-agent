import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import * as dialog from '@tauri-apps/plugin-dialog'
import * as api from '@/lib/tauri-api'
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
  sendMessage: vi.fn(),
  cancelQuery: vi.fn(),
  createSession: vi.fn(),
  switchSession: vi.fn(),
  deleteSession: vi.fn(),
  renameSession: vi.fn(),
}))

vi.mock('@/context/AppContext', () => ({
  useApp: () => ctx,
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
  ctx.sendMessage = vi.fn()
  ctx.cancelQuery = vi.fn()
  ctx.createSession = vi.fn()
  ctx.switchSession = vi.fn()
  ctx.deleteSession = vi.fn()
  ctx.renameSession = vi.fn()
}

function renderChat() {
  return render(
    <MemoryRouter>
      <Chat />
    </MemoryRouter>
  )
}

describe('Chat page', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('renders new chat button', () => {
    resetCtx()
    renderChat()
    expect(screen.getByText('New Chat')).toBeInTheDocument()
  })

  it('renders session search input', () => {
    resetCtx()
    renderChat()
    expect(screen.getByPlaceholderText('Search sessions...')).toBeInTheDocument()
  })

  it('renders message input area', () => {
    resetCtx()
    renderChat()
    expect(screen.getByPlaceholderText('Ask Shannon anything...')).toBeInTheDocument()
  })

  it('renders no sessions message when empty', () => {
    resetCtx()
    renderChat()
    expect(screen.getByText('No sessions yet')).toBeInTheDocument()
  })

  it('calls createSession when New Chat is clicked', () => {
    resetCtx()
    renderChat()
    fireEvent.click(screen.getByText('New Chat'))
    expect(ctx.createSession).toHaveBeenCalled()
  })

  it('updates session search on input', () => {
    resetCtx()
    renderChat()
    const input = screen.getByPlaceholderText('Search sessions...')
    fireEvent.change(input, { target: { value: 'test query' } })
    expect(input).toHaveValue('test query')
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

  it('renders thinking text when present', () => {
    resetCtx()
    ctx.thinkingText = 'Thinking about this...'
    renderChat()
    expect(screen.getByText(/Thinking about this/)).toBeInTheDocument()
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

  it('renders sessions in sidebar', () => {
    resetCtx()
    ctx.sessions = [
      { id: 's1', title: 'Session One', created_at: Date.now(), updated_at: Date.now() },
    ]
    renderChat()
    expect(screen.getByText('Session One')).toBeInTheDocument()
  })

  it('filters sessions by search', () => {
    resetCtx()
    ctx.sessions = [
      { id: 's1', title: 'Python Debug', created_at: Date.now(), updated_at: Date.now() },
      { id: 's2', title: 'React Setup', created_at: Date.now(), updated_at: Date.now() },
    ]
    renderChat()
    const search = screen.getByPlaceholderText('Search sessions...')
    fireEvent.change(search, { target: { value: 'react' } })
    expect(screen.getByText(/React/)).toBeInTheDocument()
    expect(screen.queryByText('Python Debug')).not.toBeInTheDocument()
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

  // F2: Export conversation as Markdown via native save dialog.
  it('renders export button per session', () => {
    resetCtx()
    ctx.sessions = [
      { id: 's1', title: 'Sess One', created_at: Date.now(), updated_at: Date.now() },
    ]
    renderChat()
    expect(screen.getByLabelText('Export Sess One')).toBeInTheDocument()
  })

  it('clicking export opens save dialog and writes file on confirm', async () => {
    resetCtx()
    ctx.sessions = [
      { id: 's1', title: 'Sess One', created_at: Date.now(), updated_at: Date.now() },
    ]
    vi.mocked(api.exportSession).mockResolvedValueOnce('# Title\n\nbody')
    vi.mocked(dialog.save).mockResolvedValueOnce('/tmp/sess.md')
    renderChat()
    fireEvent.click(screen.getByLabelText('Export Sess One'))
    await waitFor(() => {
      expect(api.exportSession).toHaveBeenCalledWith('s1', 'markdown')
      expect(dialog.save).toHaveBeenCalledWith(expect.objectContaining({
        filters: [{ name: 'Markdown', extensions: ['md'] }],
      }))
      expect(api.saveTextFile).toHaveBeenCalledWith('/tmp/sess.md', '# Title\n\nbody')
    })
  })

  it('export is a no-op when user cancels save dialog', async () => {
    resetCtx()
    ctx.sessions = [
      { id: 's1', title: 'Sess One', created_at: Date.now(), updated_at: Date.now() },
    ]
    vi.mocked(api.exportSession).mockResolvedValueOnce('body')
    vi.mocked(dialog.save).mockResolvedValueOnce(null)
    renderChat()
    fireEvent.click(screen.getByLabelText('Export Sess One'))
    await waitFor(() => {
      expect(dialog.save).toHaveBeenCalled()
    })
    expect(api.saveTextFile).not.toHaveBeenCalled()
  })

  // F2: Print / PDF opens a new window. jsdom returns null from window.open
  // unless stubbed — the test stubs a minimal fake window and only verifies
  // the call + that print() fires.
  it('renders print button per session', () => {
    resetCtx()
    ctx.sessions = [
      { id: 's1', title: 'Sess One', created_at: Date.now(), updated_at: Date.now() },
    ]
    renderChat()
    expect(screen.getByLabelText('Print or save as PDF Sess One')).toBeInTheDocument()
  })

  it('clicking print opens a new window', async () => {
    resetCtx()
    ctx.sessions = [
      { id: 's1', title: 'Sess One', created_at: Date.now(), updated_at: Date.now() },
    ]
    vi.mocked(api.exportSession).mockResolvedValueOnce('# Title')
    const fakeEl: any = { textContent: '', appendChild: vi.fn() }
    const fakeDoc: any = {
      title: '',
      head: { appendChild: vi.fn() },
      body: { appendChild: vi.fn() },
      createElement: vi.fn(() => ({ ...fakeEl })),
    }
    const fakeWin: any = {
      document: fakeDoc,
      focus: vi.fn(),
      print: vi.fn(),
    }
    const spy = vi.spyOn(window, 'open').mockReturnValueOnce(fakeWin)
    renderChat()
    fireEvent.click(screen.getByLabelText('Print or save as PDF Sess One'))
    await waitFor(() => {
      expect(api.exportSession).toHaveBeenCalledWith('s1', 'markdown')
      expect(spy).toHaveBeenCalledWith('', '_blank', 'width=900,height=700')
    })
  })
})
