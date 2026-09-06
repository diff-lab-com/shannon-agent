// P1-5 C-2 — Chat page as the WorkspaceGrid host: default look, window-mode
// guard, per-project persistence roundtrip, preset switch, and the terminal
// drawer↔grid handoff.
//
// Fix round 1: the handoff moves ONE docked TerminalPanel DOM node (never a
// portal container swap, which in React 19 remounts the subtree). The
// continuity tests below pin that contract: the terminal section element
// keeps its identity across the drawer→grid handoff, tabs/xterm survive,
// nothing re-spawns, and a fresh mount straight into a terminal layout
// reconciles tabs from `terminal_list` without spawning.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor, within } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { I18nProvider } from '@/i18n'
import Chat from '@/pages/Chat'
import * as api from '@/lib/tauri-api'
import { presetLayout, workspaceProjectKey } from '@/components/workspace/layout'
import type { TerminalInfo } from '@/types'

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

function terminalInfo(id: string, dir: string): TerminalInfo {
  return { terminalId: id, projectDir: dir, shell: '/bin/bash', startedAtMs: 1 }
}

beforeEach(() => {
  vi.clearAllMocks()
  ctx.windowSessionId = null
  ctx.sessions = []
  ctx.config = null
  ctx.xtermInstances.length = 0
  vi.mocked(api.workspaceGetLayout).mockResolvedValue(null)
  vi.mocked(api.terminalList).mockResolvedValue([])
  vi.mocked(api.terminalSpawn).mockResolvedValue({ terminalId: 'term-1' })
})

describe('Chat page workspace host', () => {
  it('renders the workspace grid + toolbar and the terminal drawer by default', async () => {
    ctx.config = projConfig
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
    ctx.config = projConfig
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

  it('drawer→grid handoff keeps the SAME TerminalPanel: tabs, xterm and no respawn survive', async () => {
    ctx.config = projConfig
    renderChat()
    await waitFor(() => expect(screen.getByTestId('workspace-toolbar')).toBeInTheDocument())

    // Open the drawer: reconciles (empty list) then spawns one PTY.
    fireEvent.click(screen.getByRole('button', { name: /Toggle terminal/ }))
    await waitFor(() => expect(screen.getByTestId('terminal-surface')).toBeInTheDocument())
    await waitFor(() => expect(api.terminalSpawn).toHaveBeenCalledTimes(1))
    const dockBefore = document.querySelector('[data-terminal-variant="drawer"]')
    expect(dockBefore).toBeTruthy()
    const termTabs = within(dockBefore as HTMLElement)
    await termTabs.findByText('proj') // dirLabel('/proj') tab
    const xtermCountBefore = ctx.xtermInstances.length
    expect(xtermCountBefore).toBeGreaterThan(0)
    // Docked inside the chat panel (drawer mode), not in a grid slot.
    expect(dockBefore!.closest('[data-testid="workspace-terminal-slot"]')).toBeNull()

    // Handoff: switch to the Build preset (adds a terminal grid panel).
    fireEvent.click(screen.getByRole('button', { name: 'Build' }))
    await waitFor(() => expect(screen.getByTestId('workspace-terminal-slot')).toBeInTheDocument())

    // The SAME section element moved containers — variant flipped in place,
    // no unmount/remount (a portal container swap would have recreated it).
    const dockAfter = await waitFor(() => document.querySelector('[data-terminal-variant="panel"]'))
    expect(dockAfter).toBe(dockBefore)
    // It now physically lives inside the grid terminal slot (moved, not
    // portaled), and exactly one dock/panel copy exists.
    expect(dockAfter!.closest('[data-testid="workspace-terminal-slot"]')).toBeTruthy()
    expect(document.querySelectorAll('[data-testid="terminal-dock"]')).toHaveLength(1)
    // State preserved: tab still listed, xterm NOT rebuilt, nothing spawned.
    expect(within(dockAfter as HTMLElement).getByText('proj')).toBeInTheDocument()
    expect(ctx.xtermInstances.length).toBe(xtermCountBefore)
    expect(api.terminalSpawn).toHaveBeenCalledTimes(1)
    // The handoff itself persisted the new layout once.
    await waitFor(() => expect(api.workspaceSetLayout).toHaveBeenCalledTimes(1))
    const [, savedLayout] = vi.mocked(api.workspaceSetLayout).mock.calls[0]
    expect(savedLayout.panels.map(p => p.kind).sort()).toEqual(['chat', 'preview', 'terminal'])
    expect(screen.queryByRole('button', { name: /Toggle terminal/ })).toBeNull()
  })

  it('fresh mount straight into a terminal layout reconciles tabs from terminal_list without spawning', async () => {
    ctx.config = projConfig
    vi.mocked(api.workspaceGetLayout).mockResolvedValue(presetLayout('build'))
    vi.mocked(api.terminalList).mockResolvedValue([terminalInfo('t-alive', '/existing')])
    renderChat()

    // The embedded panel reconciles on mount: the live PTY's tab shows up
    // with zero user interaction and zero spawns.
    expect(await screen.findByText('existing')).toBeInTheDocument()
    expect(api.terminalList).toHaveBeenCalled()
    expect(api.terminalSpawn).not.toHaveBeenCalled()
    expect(await screen.findByTestId('workspace-terminal-slot')).toBeInTheDocument()
    // Rendered as the embedded (panel) variant, not the drawer.
    expect(document.querySelector('[data-terminal-variant="panel"]')).toBeTruthy()
    expect(document.querySelector('[data-terminal-variant="drawer"]')).toBeNull()
  })

  it('a blocked keyboard move does not persist the layout', async () => {
    ctx.config = projConfig
    vi.mocked(api.workspaceGetLayout).mockResolvedValue(presetLayout('review'))
    renderChat()
    await waitFor(() => expect(screen.getByRole('button', { name: 'Panel menu: Diff' })).toBeInTheDocument())

    // The review preset tiles the grid — every move is blocked. The grid
    // skips the no-op so the host must not persist anything.
    fireEvent.click(screen.getByRole('button', { name: 'Panel menu: Diff' }))
    fireEvent.click(screen.getByRole('menuitem', { name: 'Move up' }))
    expect(api.workspaceSetLayout).not.toHaveBeenCalled()
  })

  it('reset restores the default focus layout', async () => {
    ctx.config = projConfig
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
