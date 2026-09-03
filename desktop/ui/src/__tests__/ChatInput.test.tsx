import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import ChatInput from '@/components/chat/ChatInput'
import * as api from '@/lib/tauri-api'
import { toast } from 'sonner'
import type * as ReactRouterDom from 'react-router-dom'

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

// Mock useApp hook
const mockRefreshConfig = vi.fn()
vi.mock('@/context/CatalogContext', () => ({
  useCatalog: () => ({
    config: {
      approval_mode: 'suggest',
      model: 'claude-sonnet-4-6',
      provider: 'anthropic',
      working_dir: '/home/user/projects',
    },
    models: [
      { id: 'anthropic-claude-sonnet-4-6', name: 'Claude Sonnet 4.6', provider: 'anthropic', context_window: 200000 },
      { id: 'openai-gpt-4o', name: 'GPT-4o', provider: 'openai', context_window: 128000 },
    ],
    refreshConfig: mockRefreshConfig,
  }),
}))

function renderChatInput(props: Partial<React.ComponentProps<typeof ChatInput>> = {}) {
  const defaultProps = {
    value: '',
    onChange: vi.fn(),
    onSend: vi.fn(),
    onExecuteSlash: vi.fn(),
    attachedFiles: [],
    onAttach: vi.fn(),
    onDetachAll: vi.fn(),
    disabled: false,
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
    mockRefreshConfig.mockReset()
    vi.mocked(api.configure).mockReset()
  })

  // U2: model switching moved to the global Header and the working-directory
  // picker to the composer footer — neither control lives in the strip anymore.
  it('does not render a model selector or working-directory chip (U2)', () => {
    renderChatInput()
    expect(screen.queryByLabelText('Model')).not.toBeInTheDocument()
    expect(screen.queryByLabelText('Change working directory')).not.toBeInTheDocument()
  })

  it('renders the plan-mode and permission-mode controls', () => {
    renderChatInput()
    expect(screen.getByRole('button', { name: 'Toggle plan mode' })).toBeInTheDocument()
    expect(screen.getByLabelText('Permission mode')).toBeInTheDocument()
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

    const textarea = screen.getByPlaceholderText('Ask Shannon anything...')
    fireEvent.keyDown(textarea, { key: 'Enter', code: 'Enter' })

    expect(onSend).toHaveBeenCalledTimes(1)
  })

  it('does not send on Shift+Enter', () => {
    const onSend = vi.fn()
    const onChange = vi.fn()
    renderChatInput({ value: 'Test\nmessage', onChange, onSend })

    const textarea = screen.getByPlaceholderText('Ask Shannon anything...')
    fireEvent.keyDown(textarea, { key: 'Enter', code: 'Enter', shiftKey: true })

    expect(onSend).not.toHaveBeenCalled()
  })

  // P2-5d additions — Ctrl/Cmd+Enter also sends (matches Claude.ai + the
  // task spec which asks for "Ctrl+Enter to send (configurable)").
  it('sends on Ctrl+Enter', () => {
    const onSend = vi.fn()
    renderChatInput({ value: 'a', onSend })
    const ta = screen.getByPlaceholderText('Ask Shannon anything...')
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

  it('shows a character counter once the input grows past the threshold', () => {
    const big = 'a'.repeat(2100)
    const { container } = renderChatInput({ value: big })
    const counter = container.querySelector('[role="status"][aria-live="polite"]')
    expect(counter).not.toBeNull()
    expect(counter?.textContent).toMatch(/2,100|2100/)
  })

  it('promotes the counter to error color past the soft-warn threshold', () => {
    const huge = 'a'.repeat(9000)
    const { container } = renderChatInput({ value: huge })
    const counter = container.querySelector('[role="status"]')
    expect(counter?.className).toMatch(/text-error/)
  })

  it('calls onOpenQuickFix when Quick Fix button is clicked', () => {
    const onOpenQuickFix = vi.fn()
    renderChatInput({ onOpenQuickFix })

    const quickFixButton = screen.getByTitle('Quick Fix')
    fireEvent.click(quickFixButton)

    expect(onOpenQuickFix).toHaveBeenCalledTimes(1)
  })

  it('calls onOpenEditor when Editor button is clicked', () => {
    const onOpenEditor = vi.fn()
    renderChatInput({ onOpenEditor })

    const editorButton = screen.getByTitle('Editor')
    fireEvent.click(editorButton)

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
    // Check the select has the suggest value in its hidden input
    const hiddenInput = document.querySelector('input[value="suggest"]')
    expect(hiddenInput).toBeInTheDocument()
  })

  it('shows correct icons for querying states', () => {
    renderChatInput()

    const container = screen.getByPlaceholderText('Ask Shannon anything...').closest('.group')
    expect(container).not.toHaveClass('ring-2')

    fireEvent.dragOver(container!, { dataTransfer: { files: [] } })

    // The drag state is managed internally - we just verify no crash
    expect(container).toBeInTheDocument()
  })

  it('calls onChange when textarea value changes', () => {
    const onChange = vi.fn()
    renderChatInput({ value: '', onChange })

    const textarea = screen.getByPlaceholderText('Ask Shannon anything...')
    fireEvent.change(textarea, { target: { value: 'New message' } })

    expect(onChange).toHaveBeenCalledWith('New message')
  })

  it('shows "Processing..." placeholder when querying', () => {
    renderChatInput({ isQuerying: true })

    const textarea = screen.getByPlaceholderText('Processing...')
    expect(textarea).toBeInTheDocument()
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

    fireEvent.click(screen.getByRole('button', { name: 'Toggle plan mode' }))

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
          disabled={false}
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
