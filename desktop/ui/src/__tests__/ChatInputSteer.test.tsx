// GB P2-10a — the composer's two-tier steering surface.
//
// While the session streams, the send slot becomes the QUEUE button (with
// the "will send after this turn" wording), a bolt button offers the
// interrupt-now tier, and stop remains. Keyboard: Enter = queue, Shift+Enter
// = newline, Ctrl/Cmd+Enter = interrupt now (plain send while idle).

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import ChatInput from '@/components/chat/ChatInput'
import type * as ReactRouterDom from 'react-router-dom'

vi.setConfig({ testTimeout: 60_000 })

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

vi.mock('@/context/CatalogContext', () => ({
  useCatalog: () => ({
    config: { approval_mode: 'suggest', model: 'm1', provider: 'anthropic', working_dir: '/w' },
    status: { model: 'M1', provider: 'anthropic', querying: false, message_count: 0, working_dir: '/w' },
    models: [{ id: 'm1', name: 'M1', provider: 'anthropic', context_window: 200000 }],
    refreshConfig: vi.fn(),
    refreshStatus: vi.fn(),
  }),
}))

vi.mock('@/context/SessionContext', () => ({
  useSessions: () => ({ currentSessionId: 'sess-1' }),
}))

vi.mock('@/lib/tauri-api', async () => {
  const actual = await vi.importActual<object>('@/lib/tauri-api')
  return {
    ...actual,
    configure: vi.fn().mockResolvedValue(undefined),
    getSessionModel: vi.fn().mockResolvedValue(null),
    setSessionModel: vi.fn().mockResolvedValue(undefined),
    clearSessionModel: vi.fn().mockResolvedValue(undefined),
    onWebviewFileDrop: vi.fn().mockResolvedValue(() => {}),
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
    onSteer: vi.fn(),
  }
  return render(<ChatInput {...defaultProps} {...props} />, { wrapper: I18nProvider })
}

const textarea = () => screen.getByRole('textbox', { name: 'Message' })

describe('ChatInput steering (GB P2-10a)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('idle: no queue/steer buttons, Ctrl+Enter sends', () => {
    const onSend = vi.fn()
    const onSteer = vi.fn()
    renderChatInput({ isQuerying: false, value: 'hello', onSend, onSteer })
    expect(screen.queryByLabelText('Queue message (sends after this turn)')).not.toBeInTheDocument()
    expect(screen.queryByLabelText('Send now (interrupt the current turn)')).not.toBeInTheDocument()
    fireEvent.keyDown(textarea(), { key: 'Enter', ctrlKey: true })
    expect(onSend).toHaveBeenCalledTimes(1)
    expect(onSteer).not.toHaveBeenCalled()
  })

  it('streaming: shows queue + interrupt + stop; Enter queues, Ctrl+Enter interrupts', () => {
    const onSend = vi.fn()
    const onSteer = vi.fn()
    const onCancelQuery = vi.fn()
    renderChatInput({ isQuerying: true, value: 'hello', onSend, onSteer, onCancelQuery })

    const queueBtn = screen.getByLabelText('Queue message (sends after this turn)')
    expect(queueBtn).toHaveAttribute('title', 'Queue — will send after the current turn ends')
    const steerBtn = screen.getByLabelText('Send now (interrupt the current turn)')
    expect(steerBtn).toHaveAttribute('title', expect.stringContaining('interrupts the current turn'))
    expect(screen.getByLabelText('Stop generation')).toBeInTheDocument()

    fireEvent.keyDown(textarea(), { key: 'Enter' })
    expect(onSend).toHaveBeenCalledTimes(1)
    expect(onSteer).not.toHaveBeenCalled()

    fireEvent.keyDown(textarea(), { key: 'Enter', ctrlKey: true })
    expect(onSteer).toHaveBeenCalledTimes(1)
    expect(onSend).toHaveBeenCalledTimes(1) // unchanged from the Enter above

    fireEvent.click(steerBtn)
    expect(onSteer).toHaveBeenCalledTimes(2)
  })

  it('streaming with an empty composer: only stop shows (nothing to queue)', () => {
    renderChatInput({ isQuerying: true, value: '   ' })
    expect(screen.queryByLabelText('Queue message (sends after this turn)')).not.toBeInTheDocument()
    expect(screen.queryByLabelText('Send now (interrupt the current turn)')).not.toBeInTheDocument()
    expect(screen.getByLabelText('Stop generation')).toBeInTheDocument()
  })

  it('attachments-only content still offers queue + interrupt', () => {
    const onSteer = vi.fn()
    renderChatInput({
      isQuerying: true,
      value: '',
      attachedFiles: ['/w/report.pdf'],
      onSteer,
    })
    fireEvent.click(screen.getByLabelText('Send now (interrupt the current turn)'))
    expect(onSteer).toHaveBeenCalledTimes(1)
  })

  it('Shift+Enter stays a newline in every state (no accidental send)', () => {
    const onSend = vi.fn()
    const onSteer = vi.fn()
    renderChatInput({ isQuerying: true, value: 'hello', onSend, onSteer })
    fireEvent.keyDown(textarea(), { key: 'Enter', shiftKey: true })
    expect(onSend).not.toHaveBeenCalled()
    expect(onSteer).not.toHaveBeenCalled()
  })

  it('without onSteer (legacy host), Ctrl+Enter falls back to plain send while streaming', () => {
    const onSend = vi.fn()
    renderChatInput({ isQuerying: true, value: 'hello', onSend, onSteer: undefined })
    fireEvent.keyDown(textarea(), { key: 'Enter', metaKey: true })
    expect(onSend).toHaveBeenCalledTimes(1)
  })
})
