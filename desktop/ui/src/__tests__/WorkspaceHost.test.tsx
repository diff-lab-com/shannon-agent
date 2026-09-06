// P1-5 C-2 — Chat page as the WorkspaceGrid host: default look, window-mode
// guard, per-project persistence roundtrip, preset switch, terminal
// drawer↔grid handoff.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { I18nProvider } from '@/i18n'
import Chat from '@/pages/Chat'
import * as api from '@/lib/tauri-api'
import { presetLayout, workspaceProjectKey } from '@/components/workspace/layout'

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
}))

vi.mock('@/context/ChatContext', () => ({ useChat: () => ctx }))
vi.mock('@/context/SessionContext', () => ({ useSessions: () => ctx }))
vi.mock('@/context/CatalogContext', () => ({ useCatalog: () => ctx }))

function renderChat() {
  return render(
    <I18nProvider>
      <MemoryRouter>
        <Chat />
      </MemoryRouter>
    </I18nProvider>,
  )
}

beforeEach(() => {
  vi.clearAllMocks()
  ctx.windowSessionId = null
  ctx.sessions = []
  ctx.config = null
  vi.mocked(api.workspaceGetLayout).mockResolvedValue(null)
})

describe('Chat page workspace host', () => {
  it('renders the workspace grid + toolbar and the terminal drawer by default', async () => {
    ctx.config = { working_dir: '/proj', provider: 'anthropic', api_key: 'k' }
    renderChat()

    expect(screen.getByTestId('workspace-grid')).toBeInTheDocument()
    expect(screen.getByTestId('workspace-toolbar')).toBeInTheDocument()
    // focus preset → chrome-less single chat panel.
    expect(document.querySelectorAll('[data-workspace-kind="chat"]')).toHaveLength(1)
    expect(screen.getByRole('region', { name: 'Chat' }).querySelector('[data-workspace-drag-handle]')).toBeNull()
    // Terminal lives in the drawer while the layout has no terminal panel.
    expect(screen.getByRole('button', { name: /Toggle terminal/ })).toBeInTheDocument()
    // Persistence keyed off the session cwd (frontend-computed projectKey).
    await waitFor(() =>
      expect(api.workspaceGetLayout).toHaveBeenCalledWith(workspaceProjectKey('/proj')),
    )
  })

  it('restores a saved layout (review preset adds the diff panel + chrome)', async () => {
    ctx.config = { working_dir: '/proj', provider: 'anthropic', api_key: 'k' }
    vi.mocked(api.workspaceGetLayout).mockResolvedValue(presetLayout('review'))
    renderChat()

    await waitFor(() => expect(screen.getByTestId('session-diff-panel')).toBeInTheDocument())
    // Non-default layout shows panel chrome (title bars).
    expect(screen.getByRole('region', { name: 'Chat' }).querySelector('[data-workspace-drag-handle]'))
      .toBeTruthy()
  })

  it('window mode keeps the slim single-chat view with no workspace', () => {
    ctx.windowSessionId = 'sess-1'
    renderChat()
    expect(screen.queryByTestId('workspace-grid')).toBeNull()
    expect(screen.queryByTestId('workspace-toolbar')).toBeNull()
    // No layout loading in window mode.
    expect(api.workspaceGetLayout).not.toHaveBeenCalled()
    // The terminal drawer still works there.
    expect(screen.getByRole('button', { name: /Toggle terminal/ })).toBeInTheDocument()
  })

  it('switching to the build preset persists and hands the terminal to the grid', async () => {
    ctx.config = { working_dir: '/proj', provider: 'anthropic', api_key: 'k' }
    renderChat()
    await waitFor(() => expect(screen.getByTestId('workspace-toolbar')).toBeInTheDocument())

    fireEvent.click(screen.getByRole('button', { name: 'Build' }))

    await waitFor(() => expect(api.workspaceSetLayout).toHaveBeenCalledTimes(1))
    const [, savedLayout] = vi.mocked(api.workspaceSetLayout).mock.calls[0]
    expect(savedLayout.panels.map(p => p.kind).sort()).toEqual(['chat', 'preview', 'terminal'])

    // The terminal panel is now a grid panel; the drawer handed off.
    await waitFor(() => expect(screen.getByTestId('workspace-terminal-slot')).toBeInTheDocument())
    expect(screen.getByTestId('workspace-grid').querySelector('[data-workspace-kind="terminal"]')).toBeTruthy()
    expect(screen.queryByRole('button', { name: /Toggle terminal/ })).toBeNull()
  })

  it('reset restores the default focus layout', async () => {
    ctx.config = { working_dir: '/proj', provider: 'anthropic', api_key: 'k' }
    vi.mocked(api.workspaceGetLayout).mockResolvedValue(presetLayout('review'))
    renderChat()
    await waitFor(() => expect(screen.getByTestId('workspace-toolbar')).toBeInTheDocument())

    fireEvent.click(screen.getByRole('button', { name: 'Reset workspace layout to the default' }))

    await waitFor(() => expect(api.workspaceSetLayout).toHaveBeenCalledTimes(1))
    const [, savedLayout] = vi.mocked(api.workspaceSetLayout).mock.calls[0]
    expect(savedLayout.panels).toHaveLength(1)
    expect(savedLayout.panels[0].kind).toBe('chat')
    expect(savedLayout.panels[0].rect).toEqual({ col: 1, row: 1, w: 12, h: 12 })
  })
})
