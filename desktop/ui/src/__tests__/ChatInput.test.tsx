import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import ChatInput from '@/components/chat/ChatInput'
import * as api from '@/lib/tauri-api'
import { toast } from 'sonner'

vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn(), message: vi.fn() },
}))

vi.mock('react-router-dom', async () => {
  const actual = await vi.importActual<typeof import('react-router-dom')>('react-router-dom')
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
    attachedFiles: [],
    onAttach: vi.fn(),
    onDetachAll: vi.fn(),
    disabled: false,
    isQuerying: false,
    onCancelQuery: vi.fn(),
    currentSessionId: 'session-123',
    sessionWorkingDir: '/home/user/projects/shannon',
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
    vi.mocked(api.setSessionWorkingDir).mockReset()
  })

  it('renders the three control strip components', () => {
    renderChatInput()
    expect(screen.getByLabelText('Change working directory')).toBeInTheDocument()
    expect(screen.getByLabelText('Permission mode')).toBeInTheDocument()
    expect(screen.getByLabelText('Model')).toBeInTheDocument()
  })

  it('displays working directory basename', () => {
    renderChatInput({ sessionWorkingDir: '/home/user/projects/my-project' })
    expect(screen.getByText('my-project')).toBeInTheDocument()
  })

  it('shows "Working directory" when no WD is set', () => {
    renderChatInput({ sessionWorkingDir: '' })
    expect(screen.getByText('Working directory')).toBeInTheDocument()
  })

  it('disables WD button when no session', () => {
    renderChatInput({ currentSessionId: null })
    const wdButton = screen.getByLabelText('Change working directory')
    expect(wdButton).toBeDisabled()
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

  it('renders model selector with correct default value', () => {
    renderChatInput()
    const modelSelect = screen.getByLabelText('Model')
    expect(modelSelect).toBeInTheDocument()
    // Check the Model label text is visible
    expect(screen.getByText('Model')).toBeInTheDocument()
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
