// A-22 — composer input-history recall (terminal-style ArrowUp/ArrowDown).
//
// Matrix under test (per the product ruling):
//   - empty input + ArrowUp → the most recent sent prompt; Up walks older,
//     Down walks newer, Down past the newest restores the pre-browse现场
//     (text + caret) and exits;
//   - any user typing exits history mode;
//   - entering requires an empty composer OR the caret at the very start —
//     a caret anywhere else (mid first line, later lines) keeps ArrowUp as
//     plain caret movement;
//   - the mention and slash menus keep owning the arrow keys while open;
//   - an IME composition never triggers a recall.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import React from 'react'
import { render, screen, fireEvent, waitFor, act } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import ChatInput from '@/components/chat/ChatInput'
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

vi.mock('@/context/CatalogContext', () => ({
  useCatalog: () => ({
    config: { approval_mode: 'suggest', model: 'm1', provider: 'anthropic', working_dir: '/w' },
    status: { model: 'M1', provider: 'anthropic', querying: false, message_count: 0, working_dir: '/w' },
    models: [{ id: 'm1', name: 'M1', provider: 'anthropic', context_window: 200000 }],
    refreshConfig: vi.fn(),
    refreshStatus: vi.fn(),
  }),
}))

vi.mock('@/context/SessionContext', async (importOriginal) => {
  const actual = await importOriginal<object>()
  return {
    ...actual,
    useSessions: () => ({ currentSessionId: 'sess-1' }),
  }
})

const TREE = vi.hoisted(() => [
  {
    name: 'w', path: '/w', type: 'directory' as const,
    children: [
      { name: 'app.tsx', path: '/w/app.tsx', type: 'file' as const },
      { name: 'main.rs', path: '/w/main.rs', type: 'file' as const },
    ],
  },
])

vi.mock('@/lib/tauri-api', async () => {
  const actual = await vi.importActual<object>('@/lib/tauri-api')
  return {
    ...actual,
    configure: vi.fn().mockResolvedValue(undefined),
    getSessionModel: vi.fn().mockResolvedValue(null),
    setSessionModel: vi.fn().mockResolvedValue(undefined),
    clearSessionModel: vi.fn().mockResolvedValue(undefined),
    onWebviewFileDrop: vi.fn().mockResolvedValue(() => {}),
    getFileTree: vi.fn().mockResolvedValue(TREE),
    listFileIndex: vi.fn().mockResolvedValue([]),
  }
})

const HISTORY_KEY = 'shannon.inputHistory'
const seedHistory = (entries: string[]) =>
  localStorage.setItem(HISTORY_KEY, JSON.stringify(entries))

type HostProps = Partial<React.ComponentProps<typeof ChatInput>>

/** Stateful host: mirrors what Chat.tsx does with the composer value so
 *  recalls (onChange from history nav) and typed changes both land. */
function HistoryHost(props: HostProps) {
  const [value, setValue] = React.useState('')
  return (
    <ChatInput
      value={value}
      onChange={setValue}
      onSend={() => {}}
      onExecuteSlash={() => {}}
      attachedFiles={[]}
      onAttach={() => {}}
      onDetachAll={() => {}}
      isQuerying={false}
      onCancelQuery={() => {}}
      onOpenQuickFix={() => {}}
      onOpenEditor={() => {}}
      sessionWorkingDir="/w"
      {...props}
    />
  )
}

function renderHistoryHost(props: HostProps = {}) {
  return render(<HistoryHost {...props} />, { wrapper: I18nProvider })
}

const textarea = () => screen.getByRole('textbox', { name: 'Message' })
const arrowUp = () => fireEvent.keyDown(textarea(), { key: 'ArrowUp' })
const arrowDown = () => fireEvent.keyDown(textarea(), { key: 'ArrowDown' })
/** Place the caret deterministically before a keydown. */
const caretAt = (pos: number) => textarea().setSelectionRange(pos, pos)

describe('ChatInput input history recall (A-22)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    localStorage.clear()
  })

  it('recalls the most recent sent prompt on ArrowUp from an empty composer', () => {
    seedHistory(['older one', 'newest one'])
    renderHistoryHost()
    arrowUp()
    expect(textarea()).toHaveValue('newest one')
  })

  it('walks to older entries with repeated ArrowUp and stops at the oldest', () => {
    seedHistory(['oldest', 'middle', 'newest'])
    renderHistoryHost()
    arrowUp()
    expect(textarea()).toHaveValue('newest')
    arrowUp()
    expect(textarea()).toHaveValue('middle')
    arrowUp()
    expect(textarea()).toHaveValue('oldest')
    arrowUp()
    expect(textarea()).toHaveValue('oldest')
  })

  it('ArrowDown walks back to newer entries', () => {
    seedHistory(['oldest', 'middle', 'newest'])
    renderHistoryHost()
    arrowUp()
    arrowUp()
    expect(textarea()).toHaveValue('middle')
    arrowDown()
    expect(textarea()).toHaveValue('newest')
  })

  it('Down past the newest entry restores the pre-browse现场 (empty snapshot)', () => {
    seedHistory(['newest'])
    renderHistoryHost()
    arrowUp()
    expect(textarea()).toHaveValue('newest')
    arrowDown()
    expect(textarea()).toHaveValue('')
  })

  it('Down past the newest restores a NON-empty snapshot with history mode re-armable', () => {
    seedHistory(['recalled'])
    renderHistoryHost()
    fireEvent.change(textarea(), { target: { value: 'draft text' } })
    caretAt(0)
    arrowUp()
    expect(textarea()).toHaveValue('recalled')
    arrowDown()
    expect(textarea()).toHaveValue('draft text')
    // Restoring exited history mode: ArrowUp re-enters from the top.
    caretAt(0)
    arrowUp()
    expect(textarea()).toHaveValue('recalled')
  })

  it('any user typing exits history mode (snapshot abandoned, Down is inert)', () => {
    seedHistory(['recalled entry'])
    renderHistoryHost()
    arrowUp()
    expect(textarea()).toHaveValue('recalled entry')
    // A DOM-level change = user input → browsing ends.
    fireEvent.change(textarea(), { target: { value: 'typed by hand' } })
    arrowDown()
    expect(textarea()).toHaveValue('typed by hand')
    // And ArrowUp starts a FRESH browse session from the top.
    caretAt(0)
    arrowUp()
    expect(textarea()).toHaveValue('recalled entry')
  })

  it('multi-line text: ArrowUp stays caret navigation unless the caret is at the very start', () => {
    seedHistory(['recalled'])
    renderHistoryHost()
    const ta = textarea()
    fireEvent.change(ta, { target: { value: 'hello\nworld' } })

    caretAt(8) // second line
    arrowUp()
    expect(ta).toHaveValue('hello\nworld')

    caretAt(3) // mid first line
    arrowUp()
    expect(ta).toHaveValue('hello\nworld')

    caretAt(5) // end of first line (行尾, not 行首)
    arrowUp()
    expect(ta).toHaveValue('hello\nworld')

    caretAt(0) // first line, first column → history is allowed
    arrowUp()
    expect(ta).toHaveValue('recalled')
  })

  it('an empty history never hijacks ArrowUp', () => {
    renderHistoryHost()
    arrowUp()
    expect(textarea()).toHaveValue('')
  })

  it('the @ mention menu keeps owning ArrowUp while open', async () => {
    seedHistory(['recalled'])
    renderHistoryHost()
    fireEvent.change(textarea(), { target: { value: '@' } })
    await screen.findByRole('listbox', { name: 'File mentions' })
    arrowUp()
    // History did NOT fire (the '@' text would have been replaced), and the
    // mention menu is still open for the query.
    expect(textarea()).toHaveValue('@')
    expect(screen.getByRole('listbox', { name: 'File mentions' })).toBeInTheDocument()
  })

  it('the slash menu keeps owning ArrowUp while open', () => {
    seedHistory(['recalled'])
    renderHistoryHost()
    fireEvent.change(textarea(), { target: { value: '/' } })
    expect(screen.getByRole('listbox', { name: 'Slash commands' })).toBeInTheDocument()
    arrowUp()
    expect(textarea()).toHaveValue('/')
  })

  it('an in-flight IME composition never triggers a recall', async () => {
    seedHistory(['recalled'])
    renderHistoryHost()
    const ta = textarea()
    fireEvent.compositionStart(ta)
    // Chrome ordering: keydown carries isComposing=true.
    fireEvent.keyDown(ta, { key: 'ArrowUp', isComposing: true })
    expect(ta).toHaveValue('')
    fireEvent.compositionEnd(ta)
    // Inside the just-ended grace window (Safari/Firefox ordering) a bare
    // ArrowUp is still treated as composition traffic.
    await act(async () => {})
    fireEvent.keyDown(ta, { key: 'ArrowUp' })
    expect(ta).toHaveValue('')
    // Long after the grace window, recall works again.
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 120)) })
    fireEvent.keyDown(ta, { key: 'ArrowUp' })
    await waitFor(() => expect(ta).toHaveValue('recalled'))
  })
})
