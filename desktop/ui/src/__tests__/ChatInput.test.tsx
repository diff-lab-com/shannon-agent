import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor, act } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import ChatInput from '@/components/chat/ChatInput'
import * as api from '@/lib/tauri-api'
import { toast } from 'sonner'
import type * as ReactRouterDom from 'react-router-dom'
import type { WebviewFileDropEvent } from '@/lib/tauri-api'

vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn(), message: vi.fn() },
}))

vi.mock('react-router-dom', async () => {
  const actual = await vi.importActual<typeof ReactRouterDom>('react-router-dom')
  return {
    ...actual,
    useOutletContext: () => ({ search: '' }),
    useNavigate: () => () => {},
  }
})

// B0 P0-2: capture the webview drag-drop handler ChatInput registers so
// tests can drive the Tauri v2 event stream (HTML5 DnD is dead while
// dragDropEnabled is on — there is nothing else to simulate).
const { dragDrop } = vi.hoisted(() => ({
  dragDrop: { handler: null as null | ((e: unknown) => void) },
}))

// Mock useApp hook
const mockRefreshConfig = vi.fn()
const mockRefreshStatus = vi.fn()
vi.mock('@/context/CatalogContext', () => ({
  useCatalog: () => ({
    config: {
      approval_mode: 'suggest',
      model: 'claude-sonnet-4-6',
      provider: 'anthropic',
      working_dir: '/home/user/projects',
    },
    status: { model: 'Claude Sonnet 4.6', provider: 'anthropic', querying: false, message_count: 0, working_dir: '/home/user/projects' },
    models: [
      { id: 'anthropic-claude-sonnet-4-6', name: 'Claude Sonnet 4.6', provider: 'anthropic', context_window: 200000 },
      { id: 'openai-gpt-4o', name: 'GPT-4o', provider: 'openai', context_window: 128000 },
    ],
    refreshConfig: mockRefreshConfig,
    refreshStatus: mockRefreshStatus,
  }),
}))

// SessionUsageDialog mounts on first click — its own hooks (useSessions,
// useSessionBudget → tauri-api) come from real modules, so this file must
// supply the missing pieces. The dialog only renders after the user clicks
// the entry button; we keep the data-fetch mocks returning empty values to
// keep the assertion focused on the button + dialog mount wiring.
//
// `configure` is explicitly overridden as a vi.fn() so existing tests that
// call `vi.mocked(api.configure).mockReset()` keep working (vitest's
// auto-spy only kicks in when the module is NOT mocked).
vi.mock('@/context/SessionContext', () => ({
  useSessions: () => ({ currentSessionId: 'sess-1' }),
}))
vi.mock('@/lib/tauri-api', async () => {
  const actual = await vi.importActual<object>('@/lib/tauri-api')
  return {
    ...actual,
    configure: vi.fn().mockResolvedValue(undefined),
    getSessionContextBreakdown: vi.fn().mockResolvedValue({
      totalTokens: 0, contextWindow: null,
      categories: [
        { key: 'system', tokens: 0 }, { key: 'tools', tokens: 0 }, { key: 'skills', tokens: 0 },
        { key: 'memory', tokens: 0 }, { key: 'mcp', tokens: 0 }, { key: 'conversation', tokens: 0 },
      ],
    }),
    getSessionUsage: vi.fn().mockResolvedValue({ cost_usd: 0 }),
    getSessionBudget: vi.fn().mockResolvedValue(null),
    onWebviewFileDrop: vi.fn((handler: (e: unknown) => void) => {
      dragDrop.handler = handler
      return Promise.resolve(() => { dragDrop.handler = null })
    }),
  }
})

function renderChatInput(props: Partial<React.ComponentProps<typeof ChatInput>> = {}) {
  const defaultProps = {
    value: '',
    onChange: vi.fn(),
    onSend: vi.fn(),
    onExecuteSlash: vi.fn(),
    attachedFiles: [],
    onAttach: vi.fn(),
    onDetachAll: vi.fn(),
    isQuerying: false,
    onCancelQuery: vi.fn(),
    onOpenQuickFix: vi.fn(),
    onOpenEditor: vi.fn(),
  }
  return render(<ChatInput {...defaultProps} {...props} />, { wrapper: I18nProvider })
}

describe('ChatInput', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    dragDrop.handler = null
    mockRefreshConfig.mockReset()
    mockRefreshStatus.mockReset()
    vi.mocked(api.configure).mockReset()
  })

  // P0-③ (ZCode delta): the composer carries a model chip again — synced
  // with the Header (both write config `model`/`provider`). The
  // working-directory picker stays in the composer footer (U2).
  it('renders a model chip showing the active model, but no working-directory chip', () => {
    renderChatInput()
    const chip = screen.getByLabelText('Model')
    expect(chip).toBeInTheDocument()
    // Selected value mirrors status.model by NAME — config `model` holds a
    // name, not the catalog id, and the mock catalog matches it.
    expect(chip).toHaveTextContent('Claude Sonnet 4.6')
    expect(screen.queryByLabelText('Change working directory')).not.toBeInTheDocument()
  })

  it('renders the unified permission-mode control and no separate plan toggle', () => {
    renderChatInput()
    // 2026-09 review: the standalone 计划模式 toggle was folded into the
    // permission-mode select (one surface owns `approval_mode`).
    expect(screen.getByLabelText('Permission mode')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Toggle plan mode' })).not.toBeInTheDocument()
  })

  it('calls handleSend when Send button is clicked', async () => {
    const onSend = vi.fn()
    const onChange = vi.fn()
    renderChatInput({ value: 'Hello', onChange, onSend })

    const sendButton = screen.getByLabelText('Send message')
    fireEvent.click(sendButton)

    expect(onSend).toHaveBeenCalledTimes(1)
  })

  it('disables Send button when input is empty', () => {
    renderChatInput({ value: '' })
    const sendButton = screen.getByLabelText('Send message')
    expect(sendButton).toBeDisabled()
  })

  it('renders the Voice mic button in idle state', () => {
    renderChatInput()
    expect(screen.getByLabelText('Start voice recording')).toBeInTheDocument()
  })

  it('does not render the Voice orb when idle', () => {
    const { container } = renderChatInput()
    expect(container.querySelector('[role="presentation"]')).toBeNull()
  })

  it('appends stub transcript to value after recording cycle', async () => {
    const onChange = vi.fn()
    renderChatInput({ value: '', onChange })
    const mic = screen.getByLabelText('Start voice recording')
    fireEvent.click(mic)
    expect(screen.getByLabelText('Stop recording')).toBeInTheDocument()
    fireEvent.click(screen.getByLabelText('Stop recording'))
    await waitFor(() => {
      expect(onChange).toHaveBeenCalled()
    })
    const lastCall = onChange.mock.calls[onChange.mock.calls.length - 1]
    expect(lastCall[0]).toContain('stub transcript')
  })

  it('calls onCancelQuery when Stop button is clicked', () => {
    const onCancelQuery = vi.fn()
    renderChatInput({ isQuerying: true, onCancelQuery })

    const stopButton = screen.getByLabelText('Stop generation')
    fireEvent.click(stopButton)

    expect(onCancelQuery).toHaveBeenCalledTimes(1)
  })

  it('sends message on Enter key press', () => {
    const onSend = vi.fn()
    const onChange = vi.fn()
    renderChatInput({ value: 'Test message', onChange, onSend })

    const textarea = screen.getByPlaceholderText(/Try: "Explain this repo"/)
    fireEvent.keyDown(textarea, { key: 'Enter', code: 'Enter' })

    expect(onSend).toHaveBeenCalledTimes(1)
  })

  it('does not send on Shift+Enter', () => {
    const onSend = vi.fn()
    const onChange = vi.fn()
    renderChatInput({ value: 'Test\nmessage', onChange, onSend })

    const textarea = screen.getByPlaceholderText(/Try: "Explain this repo"/)
    fireEvent.keyDown(textarea, { key: 'Enter', code: 'Enter', shiftKey: true })

    expect(onSend).not.toHaveBeenCalled()
  })

  // P2-5d additions — Ctrl/Cmd+Enter also sends (matches Claude.ai + the
  // task spec which asks for "Ctrl+Enter to send (configurable)").
  it('sends on Ctrl+Enter', () => {
    const onSend = vi.fn()
    renderChatInput({ value: 'a', onSend })
    const ta = screen.getByPlaceholderText(/Try: "Explain this repo"/)
    fireEvent.keyDown(ta, { key: 'Enter', code: 'Enter', ctrlKey: true })
    expect(onSend).toHaveBeenCalledTimes(1)
  })

  it('exposes a region landmark for the composer (role=region)', () => {
    const { container } = renderChatInput()
    expect(container.querySelector('[role="region"]')).not.toBeNull()
  })

  it('does not show a character counter for short inputs', () => {
    const { container } = renderChatInput({ value: 'short' })
    // Counter element is only rendered above the threshold.
    expect(container.querySelector('[role="status"][aria-live="polite"]')).toBeNull()
  })

  // B2 P2-9: the counter used to be role="status" aria-live="polite" — it
  // announced every keystroke past 2000 chars. It is a visual readout now;
  // a single static sr-only note about limits replaces the live region.
  it('shows a character counter once the input grows past the threshold (no live region)', () => {
    const big = 'a'.repeat(2100)
    const { container } = renderChatInput({ value: big })
    const counter = container.querySelector('span.font-mono.tabular-nums')
    expect(counter).not.toBeNull()
    expect(counter?.textContent).toMatch(/2,100|2100/)
    expect(counter).not.toHaveAttribute('aria-live')
    expect(counter).not.toHaveAttribute('role', 'status')
    expect(screen.getByText('Very long messages may exceed the model context limit.')).toBeInTheDocument()
  })

  it('promotes the counter to error color past the soft-warn threshold', () => {
    const huge = 'a'.repeat(9000)
    const { container } = renderChatInput({ value: huge })
    const counter = container.querySelector('span.font-mono.tabular-nums')
    expect(counter?.className).toMatch(/text-error/)
  })

  it('calls onOpenQuickFix from the "+" menu', () => {
    const onOpenQuickFix = vi.fn()
    renderChatInput({ onOpenQuickFix })

    // 2026-09 review: QuickFix/Editor/attach live behind one "+" menu button.
    fireEvent.click(screen.getByLabelText('Attachments and tools'))
    fireEvent.click(screen.getByRole('menuitem', { name: 'Quick Fix' }))

    expect(onOpenQuickFix).toHaveBeenCalledTimes(1)
  })

  it('calls onOpenEditor from the "+" menu', () => {
    const onOpenEditor = vi.fn()
    renderChatInput({ onOpenEditor })

    fireEvent.click(screen.getByLabelText('Attachments and tools'))
    fireEvent.click(screen.getByRole('menuitem', { name: 'Editor' }))

    expect(onOpenEditor).toHaveBeenCalledTimes(1)
  })

  it('renders attached files as chips', () => {
    renderChatInput({
      attachedFiles: ['/path/to/file1.pdf', '/path/to/file2.txt'],
    })

    expect(screen.getByText('file1.pdf')).toBeInTheDocument()
    expect(screen.getByText('file2.txt')).toBeInTheDocument()
  })

  it('renders image thumbnail for image files', () => {
    renderChatInput({
      attachedFiles: ['/path/to/screenshot.png', '/path/to/doc.pdf'],
    })

    const img = screen.getByAltText('screenshot.png')
    expect(img).toBeInTheDocument()
    expect(img).toHaveAttribute('src', 'asset://localhost/path/to/screenshot.png')

    // Non-image keeps the description icon, no <img>
    expect(screen.queryByAltText('doc.pdf')).not.toBeInTheDocument()
  })

  it('removes individual file when close button clicked', () => {
    const onAttach = vi.fn()
    renderChatInput({
      attachedFiles: ['/path/to/file1.pdf', '/path/to/file2.txt'],
      onAttach,
    })

    // Find all close icons (material-symbols-outlined with 'close' text)
    const closeIcons = screen.getAllByText('close')
    // Click the first close icon (which should be for file1.pdf)
    fireEvent.click(closeIcons[0])

    expect(onAttach).toHaveBeenCalledWith(['/path/to/file2.txt'])
  })

  it('calls onDetachAll when "Detach all" is clicked', () => {
    const onDetachAll = vi.fn()
    renderChatInput({
      attachedFiles: ['/path/to/file1.pdf', '/path/to/file2.txt'],
      onDetachAll,
    })

    const detachAllButton = screen.getByText('Detach all')
    fireEvent.click(detachAllButton)

    expect(onDetachAll).toHaveBeenCalledTimes(1)
  })

  it('renders mode selector with correct default value', () => {
    renderChatInput()
    const modeSelect = screen.getByLabelText('Permission mode')
    expect(modeSelect).toBeInTheDocument()
    // The trigger renders the selected item's label via Select.Value (plus
    // the option's icon ligature text). Case-insensitive: the ligature/icon
    // rendering differs between jsdom environments.
    expect(modeSelect).toHaveTextContent(/suggest/i)
  })

  it('shows correct icons for querying states', () => {
    renderChatInput()

    const container = screen.getByPlaceholderText(/Try: "Explain this repo"/).closest('.group')
    expect(container).not.toHaveClass('ring-2')

    fireEvent.dragOver(container!, { dataTransfer: { files: [] } })

    // The drag state is managed internally - we just verify no crash
    expect(container).toBeInTheDocument()
  })

  it('calls onChange when textarea value changes', () => {
    const onChange = vi.fn()
    renderChatInput({ value: '', onChange })

    const textarea = screen.getByPlaceholderText(/Try: "Explain this repo"/)
    fireEvent.change(textarea, { target: { value: 'New message' } })

    expect(onChange).toHaveBeenCalledWith('New message')
  })

  it('shows the queued-input placeholder (textarea stays typable) when querying', () => {
    renderChatInput({ isQuerying: true })

    // B1 §4-9 — the textarea is no longer disabled while streaming; the
    // placeholder advertises queueing instead of a hard block.
    const textarea = screen.getByPlaceholderText('Reply generating — press Enter to queue your message')
    expect(textarea).toBeInTheDocument()
    expect(textarea).not.toBeDisabled()
  })

  it('shows hourglass icon when querying', () => {
    renderChatInput({ isQuerying: true })

    // Look for hourglass_empty icon text
    const hourglassIcons = screen.getAllByText('hourglass_empty')
    expect(hourglassIcons.length).toBeGreaterThan(0)
  })

  it('shows auto_awesome icon when not querying', () => {
    renderChatInput({ isQuerying: false })

    // Look for auto_awesome icon text
    const autoAwesomeIcons = screen.getAllByText('auto_awesome')
    expect(autoAwesomeIcons.length).toBeGreaterThan(0)
  })

  it('surfaces a toast when plan mode toggle fails (was silently swallowed)', async () => {
    vi.mocked(api.configure).mockRejectedValueOnce(new Error('engine down'))
    renderChatInput()

    // The Ctrl/Cmd+Shift+P shortcut routes through the same unified toggle
    // as the mode select — a failure must still surface as a toast.
    const evt = new KeyboardEvent('keydown', { key: 'P', shiftKey: true, ctrlKey: true, bubbles: true })
    window.dispatchEvent(evt)

    await waitFor(() => {
      expect(toast.error).toHaveBeenCalledWith(
        'Failed to toggle plan mode',
        expect.objectContaining({ description: 'engine down' }),
      )
    })
  })
})

describe('ChatInput — slash-command menu', () => {
  it('opens the menu on "/" and runs the highlighted command on Enter', async () => {
    const onChange = vi.fn()
    const onExecuteSlash = vi.fn()
    const { container } = renderChatInput({ value: '/', onChange, onExecuteSlash })
    const menu = screen.getByRole('listbox', { name: 'Slash commands' })
    expect(menu).toBeInTheDocument()

    // Navigate down once (context -> cost) and run it with Enter.
    fireEvent.keyDown(container.querySelector('textarea')!, { key: 'ArrowDown' })
    fireEvent.keyDown(container.querySelector('textarea')!, { key: 'Enter' })
    expect(onExecuteSlash).toHaveBeenCalledTimes(1)
    expect(onExecuteSlash.mock.calls[0][0].name).toBe('cost')
    expect(onChange).toHaveBeenCalledWith('')
  })

  it('filters by prefix and runs a clicked entry', () => {
    const onChange = vi.fn()
    const onExecuteSlash = vi.fn()
    renderChatInput({ value: '/dif', onChange, onExecuteSlash })
    fireEvent.mouseDown(screen.getByRole('option', { selected: true }))
    expect(onExecuteSlash).toHaveBeenCalledTimes(1)
    expect(onExecuteSlash.mock.calls[0][0].name).toBe('diff')
  })

  it('hides the menu on Escape and keeps the text', () => {
    const onChange = vi.fn()
    const onSend = vi.fn()
    const view = renderChatInput({ value: '/', onChange, onSend })
    fireEvent.keyDown(view.container.querySelector('textarea')!, { key: 'Escape' })
    expect(screen.queryByRole('listbox', { name: 'Slash commands' })).toBeNull()
    expect(onSend).not.toHaveBeenCalled()
    // A new query re-opens the menu (the parent owns the value).
    view.rerender(
      <I18nProvider>
        <ChatInput
          value="/con"
          onChange={onChange}
          onSend={onSend}
          onExecuteSlash={vi.fn()}
          attachedFiles={[]}
          onAttach={vi.fn()}
          onDetachAll={vi.fn()}
          isQuerying={false}
          onCancelQuery={vi.fn()}
          onOpenQuickFix={vi.fn()}
          onOpenEditor={vi.fn()}
        />
      </I18nProvider>,
    )
    expect(screen.getByRole('listbox', { name: 'Slash commands' })).toBeInTheDocument()
  })

  it('sends unknown single tokens (e.g. pasted paths) as plain text', () => {
    const onSend = vi.fn()
    const { container } = renderChatInput({ value: '/usr/local/bin', onSend })
    expect(screen.queryByRole('listbox', { name: 'Slash commands' })).toBeNull()
    fireEvent.keyDown(container.querySelector('textarea')!, { key: 'Enter' })
    expect(onSend).toHaveBeenCalledTimes(1)
  })
})

// B2 P2-9, revised after integration review: a permanent `role="combobox"`
// on the textarea mislabels the plain multi-line composer for assistive tech
// and broke the `getByRole('textbox', { name: 'Message' })` E2E contract.
// The composer keeps its implicit textbox role; menu state (open / count /
// selection) is announced through a polite status region instead.
describe('ChatInput — slash autocomplete a11y', () => {
  it('keeps the textarea a plain textbox while the slash menu is open', () => {
    const { container } = renderChatInput({ value: '/' })
    const textarea = container.querySelector('textarea')!
    expect(textarea).not.toHaveAttribute('role')
    expect(textarea).not.toHaveAttribute('aria-expanded')
    expect(textarea).not.toHaveAttribute('aria-activedescendant')
    // The listbox itself is still exposed with usable option ids.
    const listbox = screen.getByRole('listbox', { name: 'Slash commands' })
    const optionIds = Array.from(listbox.querySelectorAll('[role="option"]')).map(o => o.id)
    optionIds.forEach(id => expect(id).toBeTruthy())
  })

  it('announces menu state and selection through the status region', () => {
    const { container } = renderChatInput({ value: '/' })
    const status = screen.getByRole('status')
    expect(status).toHaveTextContent(/selected \/context/i)
    const textarea = container.querySelector('textarea')!
    fireEvent.keyDown(textarea, { key: 'ArrowDown' })
    expect(status).not.toHaveTextContent('/context')
  })

  it('removes the status region once the menu closes', () => {
    const { container } = renderChatInput({ value: '/' })
    const textarea = container.querySelector('textarea')!
    expect(screen.getByRole('status')).toBeInTheDocument()
    fireEvent.keyDown(textarea, { key: 'Escape' })
    expect(screen.queryByRole('listbox', { name: 'Slash commands' })).toBeNull()
    expect(screen.queryByRole('status')).toBeNull()
  })

  it('drops the per-keystroke live announcements from the char counter', () => {
    const { container } = renderChatInput({ value: 'x'.repeat(2100) })
    const counter = container.querySelector('span.font-mono.tabular-nums')!
    expect(counter).toBeInTheDocument()
    expect(counter).not.toHaveAttribute('aria-live')
    expect(counter).not.toHaveAttribute('role', 'status')
    // A single static sr-only note about limits replaces the per-key spam.
    expect(screen.getByText('Very long messages may exceed the model context limit.')).toBeInTheDocument()
  })
})

// P3-③ (2026-09 三项 UX 修复 #3 收口): composer 模型 chip 旁新增
// 会话用量入口 — 单击弹出 SessionUsageDialog,弹框标题与 i18n key
// chat.input.usage.title 对应 (en: "Session usage")。
describe('ChatInput — session usage entry', () => {
  it('shows the usage button next to the model chip and opens the dialog', async () => {
    renderChatInput()
    const btn = screen.getByRole('button', { name: /session usage/i })
    expect(btn).toHaveAttribute('aria-haspopup', 'dialog')
    fireEvent.click(btn)
    // 弹框标题(chat.input.usage.title 的 en 文案)
    expect(await screen.findByText('Session usage')).toBeInTheDocument()
  })
})

// B0 P0-3 — IME composition guard: Enter/Tab while a CJK conversion is in
// flight confirm the candidate; they must never send or run a slash
// command.
describe('ChatInput — IME composition guard', () => {
  const textareaOf = (container: HTMLElement) => container.querySelector('textarea')!

  it('does not send the Enter that confirms an active composition', () => {
    const onSend = vi.fn()
    const { container } = renderChatInput({ value: 'nihao', onSend })
    const textarea = textareaOf(container)

    fireEvent.compositionStart(textarea)
    // Chrome ordering: keydown carries isComposing=true before compositionend.
    fireEvent.keyDown(textarea, { key: 'Enter', isComposing: true })
    expect(onSend).not.toHaveBeenCalled()

    fireEvent.compositionEnd(textarea)
    // Right after compositionend the Enter still lands in the grace window
    // (it may be Safari/Firefox's confirm keydown) — also swallowed.
    fireEvent.keyDown(textarea, { key: 'Enter' })
    expect(onSend).not.toHaveBeenCalled()
  })

  it('sends again once the composition grace window has passed', async () => {
    const onSend = vi.fn()
    const { container } = renderChatInput({ value: 'nihao', onSend })
    const textarea = textareaOf(container)

    fireEvent.compositionStart(textarea)
    fireEvent.compositionEnd(textarea)
    // Cross the 100ms guard window, then Enter is a real send again.
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 120)) })
    fireEvent.keyDown(textarea, { key: 'Enter' })
    expect(onSend).toHaveBeenCalledTimes(1)
  })

  it('does not send on Ctrl+Enter during composition', () => {
    const onSend = vi.fn()
    const { container } = renderChatInput({ value: 'nihao', onSend })
    const textarea = textareaOf(container)
    fireEvent.compositionStart(textarea)
    fireEvent.keyDown(textarea, { key: 'Enter', ctrlKey: true, isComposing: true })
    expect(onSend).not.toHaveBeenCalled()
  })

  it('covers the Safari/Firefox ordering where compositionend precedes the keydown', () => {
    const onSend = vi.fn()
    const { container } = renderChatInput({ value: 'nihao', onSend })
    const textarea = textareaOf(container)
    fireEvent.compositionStart(textarea)
    // Safari/Firefox: compositionend fires FIRST, so the confirming keydown
    // arrives with isComposing already false — the just-ended grace window
    // is what swallows it.
    fireEvent.compositionEnd(textarea)
    fireEvent.keyDown(textarea, { key: 'Enter' })
    expect(onSend).not.toHaveBeenCalled()
  })
})

// B0 P0-2 — drag-drop is driven by the Tauri v2 webview events (HTML5 DnD
// never fires while the webview's dragDropEnabled is on).
describe('ChatInput — Tauri v2 file drag-drop', () => {
  const drop = (payload: WebviewFileDropEvent) =>
    act(() => { dragDrop.handler?.(payload) })

  function renderWithSubscription(props: Partial<React.ComponentProps<typeof ChatInput>> = {}) {
    const view = renderChatInput(props)
    // The registration is async (Promise<unlisten>) — flush it.
    return { view, ready: waitFor(() => expect(dragDrop.handler).toBeTruthy()) }
  }

  it('registers a drag-drop listener on mount', async () => {
    const { ready } = renderWithSubscription()
    await ready
  })

  it('shows the overlay on enter/over, hides it on leave, and attaches dropped paths', async () => {
    const onAttach = vi.fn()
    const { ready } = renderWithSubscription({ onAttach })
    await ready

    drop({ type: 'enter', paths: [] })
    expect(screen.getByText('Drop files to attach')).toBeInTheDocument()

    drop({ type: 'over' })
    expect(screen.getByText('Drop files to attach')).toBeInTheDocument()

    drop({ type: 'leave' })
    expect(screen.queryByText('Drop files to attach')).not.toBeInTheDocument()

    drop({ type: 'enter', paths: [] })
    drop({ type: 'drop', paths: ['/tmp/a.png', '/tmp/b.pdf'] })
    expect(screen.queryByText('Drop files to attach')).not.toBeInTheDocument()
    await waitFor(() => expect(onAttach).toHaveBeenCalledWith(['/tmp/a.png', '/tmp/b.pdf']))
  })

  it('ignores a drop that carries no paths instead of clearing the overlay silently', async () => {
    const onAttach = vi.fn()
    const { ready } = renderWithSubscription({ onAttach })
    await ready
    drop({ type: 'drop', paths: [] })
    expect(onAttach).not.toHaveBeenCalled()
  })
})
