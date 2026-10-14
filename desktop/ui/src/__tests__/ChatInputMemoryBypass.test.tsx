// P2-5 + 02c 裁决 D4 — session-level "temporary chat" (memory bypass).
//
// The toggle folded into the composer's "+" menu (the standalone composer
// button is gone): the item keeps the `memory-bypass-toggle` testid and its
// LABEL carries the state (开/关). Still covered: the flag is read per
// focused session (never stale after a switch), the toggle flips
// optimistically with rollback on failure, and the active state is spelled
// out (banner + menu label), not just color-coded.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import ChatInput from '@/components/chat/ChatInput'
import * as api from '@/lib/tauri-api'
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

const dragDrop = { handler: null as null | ((e: unknown) => void) }

vi.mock('@/context/CatalogContext', () => ({
  useCatalog: () => ({
    config: { approval_mode: 'suggest', model: 'claude-sonnet-4-6', provider: 'anthropic' },
    status: { model: 'Claude Sonnet 4.6', provider: 'anthropic', querying: false, message_count: 0 },
    models: [
      { id: 'anthropic-claude-sonnet-4-6', name: 'Claude Sonnet 4.6', provider: 'anthropic', context_window: 200000 },
    ],
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
    setSessionModel: vi.fn().mockResolvedValue(undefined),
    clearSessionModel: vi.fn().mockResolvedValue(undefined),
    getSessionModel: vi.fn().mockResolvedValue(null),
    // P2-5 — the memory bypass flag + toggle write.
    getSessionMemoryBypass: vi.fn().mockResolvedValue(false),
    setSessionMemoryBypass: vi.fn().mockResolvedValue(undefined),
    checkAttachmentPaths: vi.fn().mockResolvedValue([]),
    registerFileIndexEntry: vi.fn().mockResolvedValue(undefined),
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
    sessionId: 'sess-1',
  }
  return render(<ChatInput {...defaultProps} {...props} />, { wrapper: I18nProvider })
}

const mockedGet = vi.mocked(api.getSessionMemoryBypass)
const mockedSet = vi.mocked(api.setSessionMemoryBypass)

/** Open the composer "+" menu (the toggle's home since 02c D4). */
async function openPlusMenu() {
  fireEvent.click(screen.getByTestId('composer-plus-menu'))
  return await screen.findByTestId('memory-bypass-toggle')
}

const itemLabel = async (expected: RegExp) => {
  const item = await openPlusMenu()
  expect(item).toHaveTextContent(expected)
  return item
}

beforeEach(() => {
  vi.clearAllMocks()
  dragDrop.handler = null
  mockedGet.mockResolvedValue(false)
  mockedSet.mockResolvedValue(undefined)
})

describe('ChatInput memory bypass (+ menu item, 02c D4)', () => {
  it('reads the flag per focused session on mount', async () => {
    renderChatInput()
    await waitFor(() => expect(mockedGet).toHaveBeenCalledWith('sess-1'))
    // Default (false): no banner; the menu item reads "off".
    expect(screen.queryByTestId('memory-bypass-banner')).not.toBeInTheDocument()
    await itemLabel(/temporary chat · off/i)
  })

  it('restores an active bypass (banner + menu label) after a session switch', async () => {
    mockedGet.mockResolvedValue(true)
    renderChatInput()
    await waitFor(() => expect(screen.getByTestId('memory-bypass-banner')).toBeInTheDocument())
    await itemLabel(/temporary chat · on/i)
  })

  it('toggles on from the + menu and writes through to the backend', async () => {
    renderChatInput()
    const item = await itemLabel(/temporary chat · off/i)
    fireEvent.click(item)
    await waitFor(() => expect(mockedSet).toHaveBeenCalledWith('sess-1', true))
    // Optimistic UI: banner without waiting for the round-trip.
    expect(screen.getByTestId('memory-bypass-banner')).toBeInTheDocument()
    // Reopening the menu shows the flipped state in the label.
    await itemLabel(/temporary chat · on/i)
  })

  it('rolls the toggle back when the backend write fails', async () => {
    mockedSet.mockRejectedValue(new Error('disk full'))
    renderChatInput()
    const item = await itemLabel(/temporary chat · off/i)
    fireEvent.click(item)
    await waitFor(() => expect(mockedSet).toHaveBeenCalledWith('sess-1', true))
    // Optimistic banner appears, then rolls back with the failed write.
    await waitFor(() => expect(screen.queryByTestId('memory-bypass-banner')).not.toBeInTheDocument())
    await itemLabel(/temporary chat · off/i)
  })

  it('re-reads the flag when the focused session changes', async () => {
    mockedGet.mockResolvedValueOnce(true).mockResolvedValueOnce(false)
    const { rerender } = renderChatInput({ sessionId: 'sess-1' })
    await waitFor(() => expect(screen.getByTestId('memory-bypass-banner')).toBeInTheDocument())
    rerender(<ChatInput {...{
      value: '', onChange: vi.fn(), onSend: vi.fn(), onExecuteSlash: vi.fn(),
      attachedFiles: [], onAttach: vi.fn(), onDetachAll: vi.fn(),
      isQuerying: false, onCancelQuery: vi.fn(), onOpenQuickFix: vi.fn(), onOpenEditor: vi.fn(),
      sessionId: 'sess-2',
    }} />)
    await waitFor(() => expect(mockedGet).toHaveBeenCalledWith('sess-2'))
    await waitFor(() => expect(screen.queryByTestId('memory-bypass-banner')).not.toBeInTheDocument())
  })

  it('the standalone composer button is gone — the + menu trigger wears the dot when active', async () => {
    mockedGet.mockResolvedValue(true)
    renderChatInput()
    await waitFor(() => expect(screen.getByTestId('memory-bypass-banner')).toBeInTheDocument())
    // No always-visible toggle button anymore (02c D4 folded it away).
    expect(screen.queryByTestId('memory-bypass-toggle')).not.toBeInTheDocument()
    // The + trigger carries the active dot.
    const plus = screen.getByTestId('composer-plus-menu').parentElement!
    expect(plus.querySelector('span.bg-secondary')).not.toBeNull()
  })
})
