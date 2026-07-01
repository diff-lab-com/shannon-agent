import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import ChatInput from '@/components/chat/ChatInput'
import * as api from '@/lib/tauri-api'

vi.mock('react-router-dom', async () => {
  const actual = await vi.importActual<typeof import('react-router-dom')>('react-router-dom')
  return {
    ...actual,
    useOutletContext: () => ({ search: '' }),
    useNavigate: () => () => {},
  }
})

const mockRefreshConfig = vi.fn()
const modeStore = { current: 'suggest' }

vi.mock('@/context/CatalogContext', () => ({
  useCatalog: () => ({
    config: {
      approval_mode: modeStore.current,
      model: 'claude-sonnet-4-6',
      provider: 'anthropic',
      working_dir: '/home/user/projects',
    },
    models: [
      { id: 'anthropic-claude-sonnet-4-6', name: 'Claude Sonnet 4.6', provider: 'anthropic', context_window: 200000 },
    ],
    refreshConfig: mockRefreshConfig,
  }),
}))

function renderWithMode(mode: string) {
  modeStore.current = mode
  return render(
    <ChatInput
      value=""
      onChange={vi.fn()}
      onSend={vi.fn()}
      attachedFiles={[]}
      onAttach={vi.fn()}
      onDetachAll={vi.fn()}
      disabled={false}
      isQuerying={false}
      onCancelQuery={vi.fn()}
      currentSessionId="s1"
      sessionWorkingDir="/home/user/projects/shannon"
      onOpenQuickFix={vi.fn()}
      onOpenEditor={vi.fn()}
    />,
    { wrapper: I18nProvider },
  )
}

describe('ChatInput — Plan Mode toggle (D4)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(api.configure).mockResolvedValue(undefined)
    mockRefreshConfig.mockReset()
  })

  it('renders the Plan mode toggle button', () => {
    renderWithMode('suggest')
    expect(screen.getByRole('button', { name: 'Toggle plan mode' })).toBeInTheDocument()
  })

  it('shows aria-pressed=false when plan mode is off', () => {
    renderWithMode('suggest')
    const toggle = screen.getByRole('button', { name: 'Toggle plan mode' })
    expect(toggle).toHaveAttribute('aria-pressed', 'false')
  })

  it('shows aria-pressed=true and active styling when plan mode is on', () => {
    renderWithMode('plan')
    const toggle = screen.getByRole('button', { name: 'Toggle plan mode' })
    expect(toggle).toHaveAttribute('aria-pressed', 'true')
    expect(toggle.className).toContain('bg-primary/10')
  })

  it('enables plan mode when clicked from suggest', async () => {
    renderWithMode('suggest')
    fireEvent.click(screen.getByRole('button', { name: 'Toggle plan mode' }))
    await waitFor(() => {
      expect(api.configure).toHaveBeenCalledWith({ key: 'approval_mode', value: 'plan' })
    })
  })

  it('disables plan mode (reverts to suggest) when clicked while plan active', async () => {
    renderWithMode('plan')
    fireEvent.click(screen.getByRole('button', { name: 'Toggle plan mode' }))
    await waitFor(() => {
      expect(api.configure).toHaveBeenCalledWith({ key: 'approval_mode', value: 'suggest' })
    })
  })

  it('shows banner when plan mode active', () => {
    renderWithMode('plan')
    expect(screen.getByText(/Plan mode active/)).toBeInTheDocument()
  })

  it('does not show banner when plan mode inactive', () => {
    renderWithMode('suggest')
    expect(screen.queryByText(/Plan mode active/)).not.toBeInTheDocument()
  })

  it('calls refreshConfig after toggling', async () => {
    renderWithMode('suggest')
    fireEvent.click(screen.getByRole('button', { name: 'Toggle plan mode' }))
    await waitFor(() => {
      expect(mockRefreshConfig).toHaveBeenCalledTimes(1)
    })
  })
})

describe('ChatInput — Plan Mode B3 enhancements', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(api.configure).mockResolvedValue(undefined)
    mockRefreshConfig.mockReset()
  })

  it('shows Exit button in banner when plan mode active', () => {
    renderWithMode('plan')
    expect(screen.getByRole('button', { name: 'Exit plan mode' })).toBeInTheDocument()
  })

  it('clicking Exit button reverts to suggest mode', async () => {
    renderWithMode('plan')
    fireEvent.click(screen.getByRole('button', { name: 'Exit plan mode' }))
    await waitFor(() => {
      expect(api.configure).toHaveBeenCalledWith({ key: 'approval_mode', value: 'suggest' })
    })
  })

  it('toggles plan mode on Ctrl+Shift+P from suggest', async () => {
    renderWithMode('suggest')
    const evt = new KeyboardEvent('keydown', { key: 'P', shiftKey: true, ctrlKey: true, bubbles: true })
    window.dispatchEvent(evt)
    await waitFor(() => {
      expect(api.configure).toHaveBeenCalledWith({ key: 'approval_mode', value: 'plan' })
    })
  })

  it('toggles plan mode off on Ctrl+Shift+P when active', async () => {
    renderWithMode('plan')
    const evt = new KeyboardEvent('keydown', { key: 'P', shiftKey: true, ctrlKey: true, bubbles: true })
    window.dispatchEvent(evt)
    await waitFor(() => {
      expect(api.configure).toHaveBeenCalledWith({ key: 'approval_mode', value: 'suggest' })
    })
  })
})
