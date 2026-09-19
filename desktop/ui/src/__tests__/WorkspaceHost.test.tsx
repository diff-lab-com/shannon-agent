// Chat page host — 2026-09 review edition.
//
// The P1-5 C-2 WorkspaceGrid + its preset toolbar (对话 / Diff / 预览) were
// RETIRED: the preset triad read as three competing "views" of one
// conversation and confused first-run users. Everything it offered has a
// single home now — RightDock tabs (documents / diff / live preview) and
// the terminal drawer (Ctrl+`). These tests pin the new contract:
//
//  * no workspace grid / toolbar, and NO per-project layout persistence;
//  * the terminal drawer still opens, reconciles `terminal_list` and
//    spawns lazily, with working xterm instances (jsdom fakes).

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor, within } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { I18nProvider } from '@/i18n'
import Chat from '@/pages/Chat'
import * as api from '@/lib/tauri-api'

const ctx = vi.hoisted(() => ({
  messages: [] as any[],
  streamingText: '',
  thinkingText: '',
  isQuerying: false,
  activeToolCalls: [] as any[],
  usage: null as any,
  sessions: [] as any[],
  currentSessionId: 'sess-1' as string | null,
  windowSessionId: null as string | null,
  error: null as string | null,
  config: null as any,
  status: null as any,
  checkpoints: [] as unknown[],
  rewindSession: vi.fn(),
  sendMessage: vi.fn(),
  cancelQuery: vi.fn(),
  createSession: vi.fn(),
  compactSession: vi.fn(),
  contextPanelOpen: false,
  setContextPanelOpen: vi.fn(),
  // xterm fakes (jsdom cannot run the real renderer): count instances so
  // continuity tests can assert terminals are never rebuilt.
  xtermInstances: [] as unknown[],
}))

vi.mock('@/context/ChatContext', () => ({ useChat: () => ctx }))
vi.mock('@/context/SessionContext', () => ({ useSessions: () => ctx }))
vi.mock('@/context/CatalogContext', () => ({ useCatalog: () => ctx }))

vi.mock('@xterm/xterm', () => ({
  Terminal: class {
    options: Record<string, unknown>
    constructor(options: Record<string, unknown>) {
      this.options = options
      ctx.xtermInstances.push(this)
    }
    loadAddon() {}
    open() {}
    write() {}
    scrollToBottom() {}
    focus() {}
    dispose() {}
    onData() { return { dispose: () => {} } }
    onResize() { return { dispose: () => {} } }
  },
}))
vi.mock('@xterm/addon-fit', () => ({
  FitAddon: class {
    fit() {}
    dispose() {}
  },
}))

function renderChat() {
  return render(
    <I18nProvider>
      <MemoryRouter>
        <Chat />
      </MemoryRouter>
    </I18nProvider>,
  )
}

const projConfig = { working_dir: '/proj', provider: 'anthropic', api_key: 'k' }

beforeEach(() => {
  vi.clearAllMocks()
  ctx.windowSessionId = null
  ctx.sessions = []
  ctx.config = null
  ctx.xtermInstances.length = 0
  vi.mocked(api.terminalList).mockResolvedValue([])
  vi.mocked(api.terminalSpawn).mockResolvedValue({ terminalId: 'term-1' })
})

describe('Chat page host (workspace retired 2026-09)', () => {
  it('renders the full-width conversation with NO workspace grid or toolbar', async () => {
    ctx.config = projConfig
    renderChat()

    expect(screen.queryByTestId('workspace-grid')).toBeNull()
    expect(screen.queryByTestId('workspace-toolbar')).toBeNull()
    // The preset switcher (对话 / Diff / 预览) is gone — no layout
    // persistence is loaded at all.
    expect(api.workspaceGetLayout).not.toHaveBeenCalled()
    expect(screen.queryByRole('button', { name: 'Preview' })).not.toBeInTheDocument()
    // The terminal drawer toggle stays.
    expect(screen.getByRole('button', { name: /Toggle terminal/ })).toBeInTheDocument()
  })

  it('opening the drawer reconciles terminal_list then spawns one PTY', async () => {
    ctx.config = projConfig
    renderChat()

    fireEvent.click(screen.getByRole('button', { name: /Toggle terminal/ }))
    await waitFor(() => expect(screen.getByTestId('terminal-surface')).toBeInTheDocument())
    await waitFor(() => expect(api.terminalSpawn).toHaveBeenCalledTimes(1))
    const dock = document.querySelector('[data-terminal-variant="drawer"]')
    expect(dock).toBeTruthy()
    await within(dock as HTMLElement).findByText('proj') // dirLabel('/proj') tab
    expect(ctx.xtermInstances.length).toBeGreaterThan(0)
  })

  it('window mode renders the same slim single-chat view', () => {
    ctx.windowSessionId = 'sess-1'
    renderChat()
    expect(screen.queryByTestId('workspace-grid')).toBeNull()
    expect(screen.queryByTestId('workspace-toolbar')).toBeNull()
    expect(api.workspaceGetLayout).not.toHaveBeenCalled()
    // The terminal drawer still works there.
    expect(screen.getByRole('button', { name: /Toggle terminal/ })).toBeInTheDocument()
  })
})
