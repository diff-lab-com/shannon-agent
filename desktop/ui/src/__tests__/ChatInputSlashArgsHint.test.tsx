// H2 P1-2 — the "/cmd args" parameter-state hint line. The autocomplete
// listbox only lives while the whole input is a bare `/token`; once args
// follow, a REAL hit against the static table or an installed skill keeps a
// fixed hint row alive above the composer. Unknown tokens (pasted paths)
// must render nothing — the hint never lies about a command existing.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen } from '@testing-library/react'
import ChatInput from '@/components/chat/ChatInput'
import type * as ReactRouterDom from 'react-router-dom'

const { navigate } = vi.hoisted(() => ({ navigate: vi.fn() }))

vi.mock('@/context/CatalogContext', () => ({
  useCatalog: () => ({
    config: { approval_mode: 'suggest', working_dir: '/tmp' },
    status: { model: 'Claude Sonnet 4.6', provider: 'anthropic', querying: false, message_count: 0, working_dir: '/tmp' },
    models: [
      { id: 'anthropic-claude-sonnet-4-6', name: 'Claude Sonnet 4.6', provider: 'anthropic', context_window: 200000 },
    ],
    refreshConfig: vi.fn(),
    refreshStatus: vi.fn(),
  }),
}))

const { savePastedText } = vi.hoisted(() => ({ savePastedText: vi.fn() }))

vi.mock('@/lib/tauri-api', async () => {
  const actual = await vi.importActual<object>('@/lib/tauri-api')
  return {
    ...actual,
    savePastedText,
    listSkills: vi.fn().mockResolvedValue([
      { name: 'pdf-reader', description: 'Reads PDFs end to end', trigger: '/pdf-reader', source: 'User', category: null },
    ]),
    checkAttachmentPaths: vi.fn().mockResolvedValue([]),
    onWebviewFileDrop: vi.fn(() => Promise.resolve(() => {})),
    registerFileIndexEntry: vi.fn().mockResolvedValue(undefined),
  }
})

vi.mock('react-router-dom', async () => {
  const actual = await vi.importActual<typeof ReactRouterDom>('react-router-dom')
  return {
    ...actual,
    useOutletContext: () => ({ search: '' }),
    useNavigate: () => navigate,
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

const hint = () => screen.queryByTestId('slash-args-hint')

describe('ChatInput — /cmd args hint line (H2 P1-2)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    savePastedText.mockResolvedValue('/home/u/.shannon/cache/pasted/1.txt')
  })

  it('shows the hint for a known command with args (description + Enter sends)', () => {
    renderChatInput({ value: '/cost this month' })
    const row = hint()
    expect(row).not.toBeNull()
    expect(row!.textContent).toContain('/cost')
    expect(row!.textContent).toContain('Tokens and cost recorded for this session')
    expect(row!.textContent).toContain('Enter to send')
    // Alias hits carry the canonical trigger.
    expect(screen.queryByRole('listbox', { name: 'Slash commands' })).toBeNull()
  })

  it('resolves an alias to the canonical command name', () => {
    renderChatInput({ value: '/save it please' })
    expect(hint()!.textContent).toContain('/export')
  })

  it('shows the hint for an installed skill with args (after the fetch lands)', async () => {
    // Skills arrive async (fetchSlashSkills on mount) — the first render
    // shows nothing, the hint lights up once the roster resolves.
    renderChatInput({ value: '/pdf-reader summarize chapter 2' })
    expect(hint()).toBeNull()
    const row = await screen.findByTestId('slash-args-hint')
    expect(row.textContent).toContain('/pdf-reader')
    expect(row.textContent).toContain('Reads PDFs end to end')
  })

  it('never shows for unknown tokens (pasted paths) or plain prompts', () => {
    renderChatInput({ value: '/usr/local/bin ls' })
    expect(hint()).toBeNull()
    renderChatInput({ value: 'plain question about /slashes' })
    expect(hint()).toBeNull()
  })

  it('yields to the menu while the input is still a bare /token', () => {
    renderChatInput({ value: '/cost' })
    expect(hint()).toBeNull()
    // A trailing space keeps the bare-token state (the menu owns it).
    expect(screen.getByRole('listbox', { name: 'Slash commands' })).toBeInTheDocument()
  })

  it('appears and disappears with the args state across rerenders', () => {
    const view = renderChatInput({ value: '/cost now' })
    expect(hint()).not.toBeNull()
    view.rerender(
      <ChatInput
        value="/cost"
        onChange={vi.fn()}
        onSend={vi.fn()}
        onExecuteSlash={vi.fn()}
        attachedFiles={[]}
        onAttach={vi.fn()}
        onDetachAll={vi.fn()}
        isQuerying={false}
        onCancelQuery={vi.fn()}
        onOpenQuickFix={vi.fn()}
        onOpenEditor={vi.fn()}
      />,
    )
    expect(hint()).toBeNull()
    expect(screen.getByRole('listbox', { name: 'Slash commands' })).toBeInTheDocument()
  })
})
