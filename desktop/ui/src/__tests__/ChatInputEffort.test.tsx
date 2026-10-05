// S3-5 (P2-19) — effort as a picker sub-tier (裁定⑪): the effort options
// expand under the EFFECTIVE model's row inside the model chip's dropdown,
// commits go through configure('effort_level') with the CANONICAL engine
// values (low|standard|high|max — the old 'medium' write is gone), the
// current tier is highlighted, and a non-default effort wears the standalone
// effort BADGE beside the chip (the `name · High` label glue is dead).
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import ChatInput from '@/components/chat/ChatInput'
import * as api from '@/lib/tauri-api'
import type * as ReactRouterDom from 'react-router-dom'

// Base UI's Select popup interactions (open / option commit) run slow under
// this repo's single-thread vitest pool — the 5s default flakes any test
// that follows a popup-opening test (same ceiling as ChatInput.test.tsx).
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

const mockRefreshConfig = vi.fn()
const mockRefreshStatus = vi.fn()

// The catalog the chip renders. `reasoning` is the S3-5 three-state wire
// bit: undefined (unknown) renders the sub-tier normally; false renders the
// honest "steers thinking models only" note. `vi.hoisted` so the hoisted
// `vi.mock` factory below can reference it.
const MOCK_MODELS = vi.hoisted(() => [
  { id: 'claude-sonnet-4-6', name: 'Claude Sonnet 4.6', provider: 'anthropic', context_window: 200000, reasoning: true },
  { id: 'text-legacy-3', name: 'Legacy Text 3', provider: 'openai', context_window: 8192, reasoning: false },
])

const catalog = vi.hoisted(() => ({
  config: {} as Record<string, unknown>,
  status: { model: 'claude-sonnet-4-6', provider: 'anthropic', querying: false, message_count: 0, working_dir: '/home/user/projects' } as Record<string, unknown>,
}))

vi.mock('@/context/CatalogContext', () => ({
  useCatalog: () => ({
    config: catalog.config,
    status: catalog.status,
    models: MOCK_MODELS,
    refreshConfig: mockRefreshConfig,
    refreshStatus: mockRefreshStatus,
  }),
}))

vi.mock('@/lib/tauri-api', async () => {
  const actual = await vi.importActual<object>('@/lib/tauri-api')
  return {
    ...actual,
    // configure mirrors the backend contract: the write lands in the config
    // the catalog hands back (refreshConfig re-reads it).
    configure: vi.fn(async (args: { key?: string; value?: string; update?: { key: string; value: string } }) => {
      const { key, value } = args?.update ?? (args as { key: string; value: string })
      catalog.config[key] = value
    }),
    setSessionModel: vi.fn().mockResolvedValue(undefined),
    clearSessionModel: vi.fn().mockResolvedValue(undefined),
    getSessionModel: vi.fn().mockResolvedValue(null),
    estimateSendCost: vi.fn().mockResolvedValue(null),
    getSessionContextBreakdown: vi.fn().mockResolvedValue({
      totalTokens: 0, contextWindow: null,
      categories: [
        { key: 'system', tokens: 0 }, { key: 'tools', tokens: 0 }, { key: 'skills', tokens: 0 },
        { key: 'memory', tokens: 0 }, { key: 'mcp', tokens: 0 }, { key: 'conversation', tokens: 0 },
      ],
    }),
    getSessionUsage: vi.fn().mockResolvedValue({ cost_usd: 0 }),
    getSessionBudget: vi.fn().mockResolvedValue(null),
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
  }
  return render(<ChatInput {...defaultProps} {...props} />)
}

// Base UI's Select in jsdom (same recipe as ChatInput.test.tsx): opening is
// a plain trigger click; committing an option needs the pointer-highlight +
// click pair. Closed portals LINGER (the data-closed leave animation never
// finishes in jsdom) — scope lookups to the freshest popup and sweep
// leftovers after each test, or a stale menu answers the next test's pick.
const currentOptions = (): HTMLElement[] => {
  const popups = document.querySelectorAll('[data-slot="select-content"]')
  const last = popups[popups.length - 1]
  return last ? Array.from(last.querySelectorAll('[role="option"]')) : []
}
const currentPopup = (): HTMLElement | null => {
  const popups = document.querySelectorAll('[data-slot="select-content"]')
  return (popups[popups.length - 1] as HTMLElement | undefined) ?? null
}
const openModelMenu = () => {
  fireEvent.click(screen.getByTestId('model-chip-trigger'))
  expect(currentOptions().length).toBeGreaterThan(0)
}
const pickOption = (el: HTMLElement) => {
  fireEvent.pointerDown(el, { button: 0 })
  fireEvent.pointerUp(el, { button: 0 })
  fireEvent.click(el)
}
const optionByTestId = (testId: string): HTMLElement => {
  const popup = currentPopup()
  if (!popup) throw new Error(`no open select popup (looking for ${testId})`)
  const el = Array.from(popup.querySelectorAll('[role="option"]'))
    .find(o => o.querySelector(`[data-testid="${testId}"]`) ?? o.dataset.testid === testId)
  if (!el) throw new Error(`option ${testId} not in the open popup`)
  return el
}

beforeEach(() => {
  vi.mocked(api.configure).mockClear()
  mockRefreshConfig.mockClear()
  mockRefreshStatus.mockClear()
  catalog.config = {}
  catalog.status = { model: 'claude-sonnet-4-6', provider: 'anthropic', querying: false, message_count: 0, working_dir: '/home/user/projects' }
})

afterEach(() => {
  document.querySelectorAll('[data-slot="select-content"]').forEach(n => n.remove())
})

describe('ChatInput effort sub-tier (S3-5)', () => {
  it('expands the effort sub-tier under the effective model row, not as a bottom section', () => {
    renderChatInput()
    openModelMenu()
    // The sub-tier header lives between model rows (inside the popup), and
    // all four canonical engine levels are present.
    expect(currentPopup()!.querySelector('[data-testid="effort-subtier-header"]')).toBeInTheDocument()
    for (const v of ['low', 'standard', 'high', 'max']) {
      expect(optionByTestId(`effort-option-${v}`)).toBeInTheDocument()
    }
    // The old detached bottom section (and its legacy 'medium' alias) are
    // gone — the canonical set is exactly the four above.
    expect(() => optionByTestId('effort-option-medium')).toThrow()
  })

  it('commits configure("effort_level") with the canonical value and refreshes config', async () => {
    renderChatInput()
    openModelMenu()
    pickOption(optionByTestId('effort-option-high'))
    await waitFor(() => {
      expect(api.configure).toHaveBeenCalledWith({ key: 'effort_level', value: 'high' })
    })
    expect(mockRefreshConfig).toHaveBeenCalled()
  })

  it('highlights the current tier and wears the badge only for non-default effort', async () => {
    catalog.config = { effort_level: 'high' }
    const view = renderChatInput()
    openModelMenu()
    // Current tier carries the checked radio glyph.
    expect(optionByTestId('effort-option-high')).toHaveTextContent('radio_button_checked')
    expect(optionByTestId('effort-option-low')).toHaveTextContent('radio_button_unchecked')
    // Non-default effort → the independent badge beside the chip.
    expect(screen.getByTestId('effort-badge')).toHaveTextContent(/deep/i)
    // The chip label is the bare model name — `name · Deep` is dead (P2-19).
    expect(screen.getByTestId('model-chip-trigger')).toHaveTextContent('Claude Sonnet 4.6')
    expect(screen.getByTestId('model-chip-trigger').textContent).not.toContain('Deep')

    // Back to the default: no badge at all (resting composer stays clean).
    // The commit writes through the config mirror (mock configure), and a
    // re-render picks the refreshed catalog up — exactly the
    // configure → refreshConfig round trip the real backend does.
    openModelMenu()
    pickOption(optionByTestId('effort-option-standard'))
    await waitFor(() => {
      expect(vi.mocked(api.configure)).toHaveBeenCalledWith({ key: 'effort_level', value: 'standard' })
    })
    view.rerender(<ChatInput value="" onChange={vi.fn()} onSend={vi.fn()} onExecuteSlash={vi.fn()} attachedFiles={[]} onAttach={vi.fn()} onDetachAll={vi.fn()} isQuerying={false} onCancelQuery={vi.fn()} onOpenQuickFix={vi.fn()} onOpenEditor={vi.fn()} />)
    expect(screen.queryByTestId('effort-badge')).not.toBeInTheDocument()
  })

  it('normalizes the legacy medium alias to standard (badge stays hidden)', () => {
    catalog.config = { effort_level: 'medium' }
    renderChatInput()
    // No crash, and the default tier renders no badge.
    expect(screen.queryByTestId('effort-badge')).not.toBeInTheDocument()
    openModelMenu()
    expect(optionByTestId('effort-option-standard')).toHaveTextContent('radio_button_checked')
  })

  it('shows no note for a reasoning-capable effective model', () => {
    renderChatInput()
    openModelMenu()
    expect(currentPopup()!.querySelector('[data-testid="effort-subtier-header"]')).toBeInTheDocument()
    expect(currentPopup()!.querySelector('[data-testid="effort-not-applicable-note"]')).not.toBeInTheDocument()
  })

  it('shows the honest not-applicable note when the effective model is KNOWN to lack reasoning', () => {
    // Known-no-reasoning effective model: the note renders under its
    // sub-tier (three-state honesty — a note, not a lockout; the engine
    // passes effort parameters through and the provider arbitrates).
    catalog.status = { model: 'text-legacy-3', provider: 'openai', querying: false, message_count: 0, working_dir: '/home/user/projects' }
    renderChatInput()
    openModelMenu()
    expect(currentPopup()!.querySelector('[data-testid="effort-not-applicable-note"]')).toBeInTheDocument()
    // …and the sub-tier stays usable (no lockout).
    expect(optionByTestId('effort-option-high')).toBeInTheDocument()
  })
})
