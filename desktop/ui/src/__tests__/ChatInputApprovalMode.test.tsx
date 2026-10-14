// 02c 裁决 D3(2026-10-14,二次修正) — the composer approval-mode control is
// a SINGLE chip showing the ACTIVE mode, opening the approval ladder as a
// dropdown. This suite pins the composer↔settings sync contract on that
// dropdown form: the trigger echoes the SHARED table's current stop (ask /
// auto-edit / plan / full-auto — `plan` is a legal engine approval_mode),
// legacy values normalize, switching commits the same
// configure('approval_mode') write the General page performs, the High-risk
// note rides the popup, and out-of-ladder engine values surface through the
// honest raw echo in the trigger instead of pretending to be a tier.
// (The former four-stop segmented control and its 1200px breakpoint are
// gone — one form at every width.)

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
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

const mockRefreshConfig = vi.fn()
const mockRefreshStatus = vi.fn()

function makeConfig(approvalMode: string) {
  return {
    approval_mode: approvalMode,
    model: 'claude-sonnet-4-6',
    provider: 'anthropic',
    working_dir: '/home/user/projects',
  }
}

let configOverride = makeConfig('suggest')

vi.mock('@/context/CatalogContext', () => ({
  useCatalog: () => ({
    get config() {
      return configOverride
    },
    status: { model: 'Claude Sonnet 4.6', provider: 'anthropic', querying: false, message_count: 0, working_dir: '/home/user/projects' },
    models: [
      { id: 'anthropic-claude-sonnet-4-6', name: 'Claude Sonnet 4.6', provider: 'anthropic', context_window: 200000 },
    ],
    refreshConfig: mockRefreshConfig,
    refreshStatus: mockRefreshStatus,
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
  return render(<ChatInput {...defaultProps} {...props} />, { wrapper: I18nProvider })
}

/** The active-mode chip (Base UI Select trigger). One form at every width. */
function modeChip(): HTMLElement {
  return screen.getByTestId('approval-mode-pill')
}

// Base UI's Select in jsdom (same recipe as ChatInputEffort.test.tsx):
// opening is a plain trigger click; committing an option needs the
// pointer-highlight + click pair. Closed portals LINGER (the data-closed
// leave animation never finishes in jsdom) — scope lookups to the freshest
// popup and sweep leftovers after each test.
const currentPopup = (): HTMLElement | null => {
  const popups = document.querySelectorAll('[data-slot="select-content"]')
  return (popups[popups.length - 1] as HTMLElement | undefined) ?? null
}
const openMenu = () => {
  fireEvent.click(modeChip())
  expect(currentPopup()).not.toBeNull()
}
const pickOption = (el: HTMLElement) => {
  fireEvent.pointerDown(el, { button: 0 })
  fireEvent.pointerUp(el, { button: 0 })
  fireEvent.click(el)
}
const optionByName = (name: RegExp): HTMLElement => {
  const popup = currentPopup()
  if (!popup) throw new Error('no open select popup')
  const el = Array.from(popup.querySelectorAll('[role="option"]')).find(o => name.test(o.textContent ?? ''))
  if (!el) throw new Error(`option ${name} not in the open popup`)
  return el
}

beforeEach(() => {
  vi.clearAllMocks()
  configOverride = makeConfig('ask')
  vi.mocked(api.configure).mockReset()
  vi.mocked(api.configure).mockResolvedValue(undefined)
})

afterEach(() => {
  document.querySelectorAll('[data-slot="select-content"]').forEach(n => n.remove())
})

describe('ChatInput approval-mode chip + dropdown (02c 裁决 D3)', () => {
  it('renders the SHARED table label for the current config mode (auto_edit → Auto Edit)', () => {
    configOverride = makeConfig('auto_edit')
    renderChatInput()
    // Legacy 'auto_edit' normalizes to the auto-edit ladder tier — the chip
    // echoes it, proving the composer reads the same table the settings
    // page writes.
    expect(modeChip()).toHaveTextContent(/auto edit/i)
  })

  it('suggest renders as the Ask tier (legacy alias of the ladder base)', () => {
    renderChatInput()
    expect(modeChip()).toHaveTextContent(/^.*ask/i)
  })

  it('renders unknown engine values honestly via the raw echo — no tier claimed', () => {
    configOverride = makeConfig('mystery-mode')
    renderChatInput()
    // A value this UI does not manage shows raw in the chip instead of
    // dressing itself up as a tier the user could meaningfully pick.
    expect(modeChip()).toHaveTextContent('mystery-mode')
  })

  it('legacy confirm normalizes to the Ask tier (no fake raw readout)', () => {
    configOverride = makeConfig('confirm')
    renderChatInput()
    expect(modeChip()).toHaveTextContent(/ask/i)
  })

  it('the dropdown lists exactly the four ladder stops plus the high-risk note', () => {
    renderChatInput()
    openMenu()
    const popup = currentPopup()!
    const options = Array.from(popup.querySelectorAll('[role="option"]'))
    expect(options).toHaveLength(4)
    expect(options.map(o => o.textContent)).toEqual([
      expect.stringMatching(/ask/i),
      expect.stringMatching(/auto edit/i),
      expect.stringMatching(/full/i),
      expect.stringMatching(/plan/i),
    ])
    // GB P2-4: the danger note travels with the switcher.
    expect(popup.textContent).toContain('High-risk actions')
  })

  it('picking a tier commits configure(approval_mode) + refreshConfig — the General-page write', async () => {
    renderChatInput()
    openMenu()
    pickOption(optionByName(/auto edit/i))
    // The commit chain is async (pick → await configure → await
    // refreshConfig). Order-specific: refreshConfig is only asserted after
    // configure landed.
    await waitFor(() => expect(api.configure).toHaveBeenCalledWith({ key: 'approval_mode', value: 'auto-edit' }), { timeout: 15_000 })
    await waitFor(() => expect(mockRefreshConfig).toHaveBeenCalled(), { timeout: 15_000 })
  })

  it('the plan option writes the engine-legal plan value', async () => {
    renderChatInput()
    openMenu()
    pickOption(optionByName(/plan/))
    await waitFor(() => expect(api.configure).toHaveBeenCalledWith({ key: 'approval_mode', value: 'plan' }), { timeout: 15_000 })
    // The standalone plan-mode toggle is long gone — one surface owns the key.
    expect(screen.queryByRole('button', { name: 'Toggle plan mode' })).not.toBeInTheDocument()
  })

  it('an out-of-table current value never hides the listed tiers (plan_ro → Strict echo)', () => {
    configOverride = makeConfig('plan_ro')
    renderChatInput()
    // Legacy plan_ro normalizes to the readonly EXPERT mode — labeled
    // honestly (Strict) in the chip, while the dropdown still lists all
    // four stops pickable.
    expect(modeChip()).toHaveTextContent(/strict/i)
    openMenu()
    expect(currentPopup()!.querySelectorAll('[role="option"]')).toHaveLength(4)
    expect(currentPopup()!.textContent).toContain('High-risk actions')
  })

  it('the segmented control is gone — no segment testids survive (02c D3)', () => {
    renderChatInput()
    expect(screen.queryByTestId('approval-mode-segment-ask')).toBeNull()
    expect(screen.queryByTestId('approval-mode-offladder-badge')).toBeNull()
    expect(document.querySelectorAll('[role="radiogroup"][aria-label="Permission mode"]')).toHaveLength(0)
  })
})

// R2 audit G2/I1 (02c A2 可选加分) — Shift+Tab walks the approval ladder
// without opening the dropdown: ask → auto-edit → plan → full-auto, then
// wraps. Same configure('approval_mode') + refreshConfig write the pill
// commits, with a toast naming the NEW tier as the feedback channel.
describe('ChatInput approval-mode Shift+Tab cycle (R2 G2/I1)', () => {
  it('cycles ask → auto-edit → plan → full-auto → ask, writing configure each step', async () => {
    configOverride = makeConfig('ask')
    // The write lands in the config the catalog hands back (same mirror
    // recipe as ChatInputEffort) so the NEXT keystroke reads the new tier.
    vi.mocked(api.configure).mockImplementation(async (update?: { key: string; value: string }) => {
      if (update?.key === 'approval_mode') configOverride = makeConfig(update.value)
    })
    const view = renderChatInput()
    const textarea = screen.getByRole('textbox')
    for (const next of ['auto-edit', 'plan', 'full-auto', 'ask']) {
      fireEvent.keyDown(textarea, { key: 'Tab', shiftKey: true })
      await waitFor(
        () => expect(api.configure).toHaveBeenCalledWith({ key: 'approval_mode', value: next }),
        { timeout: 15_000 },
      )
      await waitFor(() => expect(mockRefreshConfig).toHaveBeenCalled(), { timeout: 15_000 })
      // The real refreshConfig re-reads the catalog and re-renders; the mock
      // needs the re-render spelled out (same recipe as ChatInputEffort) or
      // the next keystroke would read the stale render's config closure.
      view.rerender(<ChatInput value="" onChange={vi.fn()} onSend={vi.fn()} onExecuteSlash={vi.fn()} attachedFiles={[]} onAttach={vi.fn()} onDetachAll={vi.fn()} isQuerying={false} onCancelQuery={vi.fn()} onOpenQuickFix={vi.fn()} onOpenEditor={vi.fn()} />)
    }
  })

  it('toasts the NEW tier as the feedback channel (no alert)', async () => {
    const { toast } = await import('sonner')
    renderChatInput()
    fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Tab', shiftKey: true })
    await waitFor(() => expect(toast.success).toHaveBeenCalledWith('Permission mode: Auto Edit'), { timeout: 15_000 })
  })

  it('plain Tab (no shift) never cycles the ladder', () => {
    renderChatInput()
    fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Tab' })
    expect(api.configure).not.toHaveBeenCalled()
  })

  it('an out-of-ladder current value (expert mode) enters the loop at ask', async () => {
    configOverride = makeConfig('bypass_permissions')
    renderChatInput()
    fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Tab', shiftKey: true })
    await waitFor(() => expect(api.configure).toHaveBeenCalledWith({ key: 'approval_mode', value: 'ask' }), { timeout: 15_000 })
  })

  it('Shift+Tab with the slash menu open commits the menu, not the ladder', () => {
    renderChatInput({ value: '/' })
    fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Tab', shiftKey: true })
    // The menu commit path above owns the keystroke — no configure write.
    expect(api.configure).not.toHaveBeenCalled()
  })
})
