// ComposerSwitchers — placement contracts for the composer's switcher row.
//
// 02c 收敛裁决(2026-10-14) — D1: the PhaseTierSwitcher is GONE from the
// composer; Settings → 模型 (PhaseTierSection, phase-tier-section testid)
// is the plan/act tier pair's only home, so the composer can no longer
// write a GLOBAL config from a per-chat surface, and the "继承" default
// that leaked the precedence chain into UI language is out of the chat
// flow entirely. D3: the approval-mode pill renders as the single
// active-mode chip + dropdown. This suite pins the absences and the
// surviving surfaces; the phase-tier WRITE coverage lives with the
// Settings surface (model-profiles e2e + ModelsSettings tests).

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen } from '@testing-library/react'
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
    currentGitBranch: vi.fn().mockResolvedValue(null),
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
  } as ComposerContextValue
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

describe('ComposerSwitchers — placement (02c 收敛后)', () => {
  it('the composer no longer carries the phase-tier switcher (D1 — Settings is the only home)', () => {
    renderPanel(composerValue())
    expect(screen.queryByTestId('phase-tier-switcher')).not.toBeInTheDocument()
    expect(screen.queryByTestId('phase-tier-menu')).not.toBeInTheDocument()
  })

  it('the approval mode renders as the single active-mode chip (D3), the usage chip beside the model chip', () => {
    renderPanel(composerValue())
    expect(screen.getByTestId('approval-mode-pill')).toBeInTheDocument()
    expect(screen.getByTestId('usage-chip')).toBeInTheDocument()
    expect(screen.getByTestId('effort-gauge')).toBeInTheDocument()
    // 缓期批 3 收敛: the execution-mode twin is still retired from the
    // composer — profile activation is a Settings concern.
    expect(screen.queryByTestId('execution-mode-switcher')).not.toBeInTheDocument()
  })

  it('the context-% chip stays icon-only while no usage payload has a resolved window (D4 honesty)', () => {
    renderPanel(composerValue())
    const chip = screen.getByTestId('usage-chip')
    expect(chip).toBeInTheDocument()
    // ctx.usage is null → no engine-resolved window → NO percentage, ever.
    expect(chip.textContent).not.toMatch(/%/)
  })

  it('the context-% chip shows the real percent once usage reports a resolved window (D4)', () => {
    ctx.usage = { input_tokens: 38_000, output_tokens: 36_000, context_total: 200_000, cost_usd: 0.0872 }
    renderPanel(composerValue())
    // 74k/200k → 37% (same math the status-bar segment used).
    expect(screen.getByTestId('usage-chip')).toHaveTextContent('37%')
  })
})
