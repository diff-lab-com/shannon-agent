// ComposerPanel (src/pages/chat/ComposerPanel.tsx) — zero-coverage until R2.
// Shallow-surface contract only (deep composer interaction lives in
// ChatInput's own suites): the edit banner's show/hide + cancel, the slash
// result card passthrough with its dismiss, the QueueChips mount, and the
// working-directory footer's session binding.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'

import ComposerPanel from '@/pages/chat/ComposerPanel'
import { ComposerContext, type ComposerContextValue } from '@/pages/chat/ComposerContext'
import { useChat } from '@/context/ChatContext'
import { useSessions } from '@/context/SessionContext'
import { useCatalog } from '@/context/CatalogContext'

const ctx = vi.hoisted(() => ({
  isQuerying: false,
  cancelQuery: vi.fn(),
  usage: null as any,
  sessions: [] as any[],
  currentSessionId: 'session-1' as string | null,
  promptQueue: [] as any[],
  removeQueuedPrompt: vi.fn(),
  moveQueuedPrompt: vi.fn(),
  config: null as any,
}))

vi.mock('@/context/ChatContext', () => ({ useChat: () => ctx }))
vi.mock('@/context/SessionContext', async (importOriginal) => {
  const actual = await importOriginal<object>()
  return { ...actual, useSessions: () => ctx }
})
vi.mock('@/context/CatalogContext', () => ({ useCatalog: () => ctx }))

const noop = () => {}

function composerValue(patch: Partial<ComposerContextValue> = {}): ComposerContextValue {
  return {
    input: '',
    setInput: noop,
    handleSend: noop,
    handleSteer: noop,
    attachedFiles: [],
    handleAttach: noop,
    handleDetachAll: noop,
    executeSlash: noop,
    slashResult: null,
    dismissSlashResult: vi.fn(),
    editing: null,
    cancelEdit: vi.fn(),
    ...patch,
  }
}

function renderPanel(value: ComposerContextValue) {
  return render(
    <MemoryRouter>
      <ComposerContext.Provider value={value}>
        <ComposerPanel setQuickFixOpen={noop} setEditorOpen={noop} />
      </ComposerContext.Provider>
    </MemoryRouter>,
  )
}

beforeEach(() => {
  ctx.isQuerying = false
  ctx.sessions = [{ id: 'session-1', title: 'S', working_dir: '/home/alice/code/myproject' }]
  ctx.currentSessionId = 'session-1'
  ctx.promptQueue = []
  ctx.config = { working_dir: '/fallback/dir' }
})

describe('ComposerPanel — edit banner (B1 §4-8)', () => {
  it('identifies the message under edit and offers the escape hatch', () => {
    const cancelEdit = vi.fn()
    renderPanel(composerValue({
      editing: {
        index: 2,
        turnIndex: 1,
        content: 'original text',
        timestamp: new Date('2026-10-02T09:05:00').getTime(),
        attachmentPaths: [],
        draft: { text: 'pre-edit draft', attachments: [] },
      },
      cancelEdit,
    }))
    const banner = screen.getByTestId('edit-banner')
    expect(banner).toHaveTextContent('Editing your message from')
    fireEvent.click(screen.getByRole('button', { name: 'Cancel editing and restore draft' }))
    expect(cancelEdit).toHaveBeenCalledTimes(1)
  })

  it('hides the banner while not editing', () => {
    renderPanel(composerValue())
    expect(screen.queryByTestId('edit-banner')).toBeNull()
  })
})

describe('ComposerPanel — slash result passthrough', () => {
  it('renders the card for the current slash result and dismisses through it', () => {
    const dismissSlashResult = vi.fn()
    renderPanel(composerValue({
      slashResult: { kind: 'error', messageKey: 'chat.error.cancelFailed' },
      dismissSlashResult,
    }))
    expect(screen.getByText('Command failed')).toBeInTheDocument()
    expect(screen.getByText('Failed to cancel query')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }))
    expect(dismissSlashResult).toHaveBeenCalledTimes(1)
  })

  it('renders no card without a result', () => {
    renderPanel(composerValue())
    expect(screen.queryByText('Command failed')).toBeNull()
  })
})

describe('ComposerPanel — queue chips mount (B1 §4-9)', () => {
  it('surfaces queued prompts above the input', () => {
    ctx.promptQueue = [{ id: 1, text: 'queued while streaming', attachments: [] }]
    renderPanel(composerValue())
    expect(screen.getByTestId('prompt-queue')).toBeInTheDocument()
    expect(screen.getByText('queued while streaming')).toBeInTheDocument()
  })
})

describe('ComposerPanel — working directory footer', () => {
  it('renders the session working dir as a breadcrumb', () => {
    renderPanel(composerValue())
    const button = screen.getByRole('button', { name: 'Change working directory for this chat' })
    expect(button).toBeEnabled()
    expect(screen.getByText('…/code/myproject')).toBeInTheDocument()
  })

  it('disables the picker without a session', () => {
    ctx.currentSessionId = null
    ctx.config = null
    renderPanel(composerValue())
    expect(screen.getByRole('button', { name: 'Change working directory for this chat' })).toBeDisabled()
    expect(screen.getByText('Not set')).toBeInTheDocument()
  })
})
