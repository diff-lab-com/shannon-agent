// ComposerSwitchers — Aurora redesign 2026-10 (02-chat.html 三要素归位):
// the PhaseTierSwitcher moved from the chat Header into the composer's
// compose row. This suite pins the composer placement and keeps the
// phase-tier write coverage that used to live in Header.test's P1-3
// describe (the Header half pins the ABSENCE).
//
// 缓期批 3 收敛: the execution-mode twin is GONE from the composer — it
// activated permission profiles, and backend 8803a519d removed the
// activation → approval_mode overwrite, so a composer control that could
// no longer move the send-time mode was dishonest chrome. Rule presets
// (规则预设) live in Settings → 权限与安全 (PermissionsSettings tests own
// the activation coverage); the composer's only mode surface is the
// approval-mode pill (ChatInputApprovalMode.test.tsx owns that).

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
  ctx.config = { approval_mode: 'suggest', working_dir: '/home/alice/code/myproject' }
  ctx.refreshConfig = vi.fn()
  vi.mocked(api.configure).mockClear()
})

describe('ComposerSwitchers — placement (三要素归位)', () => {
  it('renders the phase-tier switcher inside the composer and the mode pill beside it', () => {
    renderPanel(composerValue())
    // The compose row carries the phase-tier switcher (testid unchanged from
    // the header era — e2e anchors by testid, not container).
    expect(screen.getByTestId('phase-tier-switcher')).toBeInTheDocument()
    // 缓期批 3 收敛: the execution-mode twin is retired from the composer —
    // profile activation is a Settings concern now.
    expect(screen.queryByTestId('execution-mode-switcher')).not.toBeInTheDocument()
  })
})

describe('ComposerSwitchers — phase-tier write', () => {
  it('writes act_tier through the global config on an Act pick', async () => {
    renderPanel(composerValue())
    fireEvent.click(screen.getByTestId('phase-tier-switcher'))
    const menu = await screen.findByTestId('phase-tier-menu')
    // The approval-mode segmented control (Aurora 2026-10 裁决 B1) also
    // renders a radiogroup in the composer — select the phase groups by
    // accessible name, not positional index.
    const actGroup = screen.getByRole('radiogroup', { name: 'Execution tier' })
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
