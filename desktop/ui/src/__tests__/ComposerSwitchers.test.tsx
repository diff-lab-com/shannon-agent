// ComposerSwitchers — Aurora redesign 2026-10 (02-chat.html 三要素归位):
// the ExecutionModeSwitcher + PhaseTierSwitcher moved from the chat Header
// into the composer's compose row. This suite pins the composer placement
// and relocates the activation behavior coverage that used to live in
// Header.test's P1-3 describe (the Header half now pins the ABSENCE).

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { I18nProvider } from '@/i18n'

import ComposerPanel from '@/pages/chat/ComposerPanel'
import { ComposerContext, type ComposerContextValue } from '@/pages/chat/ComposerContext'
import * as api from '@/lib/tauri-api'
import type * as TauriApiModule from '@/lib/tauri-api'

const ctx = vi.hoisted(() => ({
  isQuerying: false,
  isCancelInFlight: false,
  cancelQuery: vi.fn(),
  usage: null as any,
  sessions: [] as any[],
  currentSessionId: 'session-1' as string | null,
  promptQueue: [] as any[],
  config: null as any,
  refreshConfig: vi.fn(),
}))

vi.mock('@/context/ChatContext', () => ({ useChat: () => ctx }))
vi.mock('@/context/SessionContext', async (importOriginal) => {
  const actual = await importOriginal<object>()
  return { ...actual, useSessions: () => ctx }
})
vi.mock('@/context/CatalogContext', () => ({ useCatalog: () => ctx }))

vi.mock('@/lib/tauri-api', async (importOriginal) => {
  const actual = await importOriginal<typeof TauriApiModule>()
  return {
    ...actual,
    activatePermissionProfile: vi.fn().mockResolvedValue({ active: 'strict', approval_mode: 'ask' }),
    configure: vi.fn().mockResolvedValue(undefined),
  }
})

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
    <I18nProvider>
      <MemoryRouter>
        <ComposerContext.Provider value={value}>
          <ComposerPanel setQuickFixOpen={noop} setEditorOpen={noop} />
        </ComposerContext.Provider>
      </MemoryRouter>
    </I18nProvider>,
  )
}

beforeEach(() => {
  ctx.isQuerying = false
  ctx.isCancelInFlight = false
  ctx.sessions = [{ id: 'session-1', title: 'S', working_dir: '/home/alice/code/myproject' }]
  ctx.currentSessionId = 'session-1'
  ctx.promptQueue = []
  ctx.config = { active_permission_profile: 'balanced', approval_mode: 'suggest', working_dir: '/home/alice/code/myproject' }
  ctx.refreshConfig = vi.fn()
  vi.mocked(api.activatePermissionProfile).mockClear()
  vi.mocked(api.configure).mockClear()
})

describe('ComposerSwitchers — placement (三要素归位)', () => {
  it('renders the execution-mode and phase-tier switchers inside the composer', () => {
    renderPanel(composerValue())
    // The compose row now carries both switchers (testids unchanged from
    // the header era — e2e anchors by testid, not container).
    expect(screen.getByTestId('execution-mode-switcher')).toBeInTheDocument()
    expect(screen.getByTestId('phase-tier-switcher')).toBeInTheDocument()
    // Compact variants still announce the full state.
    expect(screen.getByRole('button', { name: 'Execution mode: Balanced. Press to change.' })).toBeInTheDocument()
  })

  it('shows a custom profile name on the custom tier', () => {
    ctx.config = { ...ctx.config, active_permission_profile: 'research-mode' }
    renderPanel(composerValue())
    expect(screen.getByRole('button', { name: 'Execution mode: Custom: research-mode. Press to change.' })).toBeInTheDocument()
  })
})

describe('ComposerSwitchers — execution-mode activation (relocated from Header P1-3)', () => {
  it('dispatches activate_permission_profile and refreshes config on tier switch', async () => {
    renderPanel(composerValue())
    fireEvent.click(screen.getByRole('button', { name: 'Execution mode: Balanced. Press to change.' }))
    fireEvent.click(await screen.findByRole('option', { name: 'Strict' }))
    await waitFor(() => {
      expect(api.activatePermissionProfile).toHaveBeenCalledWith('strict')
      expect(ctx.refreshConfig).toHaveBeenCalled()
    })
  })

  it('marks the active tier aria-selected in the menu', async () => {
    renderPanel(composerValue())
    fireEvent.click(screen.getByRole('button', { name: 'Execution mode: Balanced. Press to change.' }))
    const balanced = await screen.findByRole('option', { name: 'Balanced' })
    expect(balanced).toHaveAttribute('aria-selected', 'true')
    expect(await screen.findByRole('option', { name: 'Strict' })).toHaveAttribute('aria-selected', 'false')
  })
})

describe('ComposerSwitchers — phase-tier write', () => {
  it('writes act_tier through the global config on an Act pick', async () => {
    renderPanel(composerValue())
    fireEvent.click(screen.getByTestId('phase-tier-switcher'))
    const menu = await screen.findByTestId('phase-tier-menu')
    // Second radiogroup = the Act phase (plan renders first).
    const actGroup = screen.getAllByRole('radiogroup')[1]!
    const fast = Array.from(actGroup.querySelectorAll('[role="radio"]')).find(r =>
      r.textContent?.includes('Fast'),
    )
    expect(fast).toBeTruthy()
    fireEvent.click(fast!)
    await waitFor(() => {
      expect(api.configure).toHaveBeenCalledWith({ key: 'act_tier', value: 'fast' })
    })
    void menu
  })
})
