// Office Wave 1 A1a — the composer must say out loud when an attachment's
// content will NOT be parsed (.docx/.xlsx/…). The chip still attaches and
// the path is still sent; only the notice is new. See ChatInput.tsx
// UNPARSED_EXTENSIONS.
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor, act } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import ChatInput, { UNPARSED_EXTENSIONS, pathExtension } from '@/components/chat/ChatInput'
import type * as ReactRouterDom from 'react-router-dom'
import type { WebviewFileDropEvent } from '@/lib/tauri-api'

const UNSUPPORTED_MAIN = "This file type isn't parsed yet — its content was NOT sent to the model."
const UNSUPPORTED_HINT = 'Convert to PDF/TXT/Markdown, or install Python 3 so built-in skills can read office files.'

// B0 P0-2 pattern (ChatInput.test.tsx): capture the drag-drop handler so the
// Tauri v2 drop flow can be driven from tests.
const { dragDrop } = vi.hoisted(() => ({
  dragDrop: { handler: null as null | ((e: unknown) => void) },
}))

vi.mock('@/context/CatalogContext', () => ({
  useCatalog: () => ({
    config: { approval_mode: 'suggest', model: 'claude-sonnet-4-6', provider: 'anthropic', working_dir: '/tmp' },
    status: { model: 'Claude Sonnet 4.6', provider: 'anthropic', querying: false, message_count: 0, working_dir: '/tmp' },
    models: [
      { id: 'anthropic-claude-sonnet-4-6', name: 'Claude Sonnet 4.6', provider: 'anthropic', context_window: 200000 },
    ],
    refreshConfig: vi.fn(),
    refreshStatus: vi.fn(),
  }),
}))

vi.mock('@/lib/tauri-api', async () => {
  const actual = await vi.importActual<object>('@/lib/tauri-api')
  return {
    ...actual,
    configure: vi.fn().mockResolvedValue(undefined),
    onWebviewFileDrop: vi.fn((handler: (e: unknown) => void) => {
      dragDrop.handler = handler
      return Promise.resolve(() => { dragDrop.handler = null })
    }),
  }
})

vi.mock('react-router-dom', async () => {
  const actual = await vi.importActual<typeof ReactRouterDom>('react-router-dom')
  return {
    ...actual,
    useOutletContext: () => ({ search: '' }),
    useNavigate: () => () => {},
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

describe('ChatInput — unsupported attachment notice (A1a)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    dragDrop.handler = null
  })

  it('keeps the declared office extensions unparsed (doc/xls/ppt family)', () => {
    for (const ext of ['doc', 'docx', 'xls', 'xlsx', 'ppt', 'pptx', 'odt', 'rtf']) {
      expect(UNPARSED_EXTENSIONS.has(ext), ext).toBe(true)
    }
    // Supported types must NOT be flagged.
    for (const ext of ['pdf', 'txt', 'md', 'csv', 'png']) {
      expect(UNPARSED_EXTENSIONS.has(ext), ext).toBe(false)
    }
  })

  it('extracts a lowercase extension and treats dotfiles as extension-less', () => {
    expect(pathExtension('/tmp/Report.DOCX')).toBe('docx')
    expect(pathExtension('C:\\docs\\q3.xlsx')).toBe('xlsx')
    expect(pathExtension('/tmp/notes.txt')).toBe('txt')
    expect(pathExtension('/tmp/.gitignore')).toBe('')
    expect(pathExtension('/tmp/noext')).toBe('')
  })

  it('shows the notice when a .docx is attached (chip still present)', () => {
    renderChatInput({ attachedFiles: ['/home/u/Downloads/report.docx'] })
    expect(screen.getByText('report.docx')).toBeInTheDocument() // chip unchanged
    expect(screen.getByText(UNSUPPORTED_MAIN)).toBeInTheDocument()
    expect(screen.getByText(UNSUPPORTED_HINT)).toBeInTheDocument()
  })

  it('does not show the notice for a plain .txt attachment', () => {
    renderChatInput({ attachedFiles: ['/tmp/notes.txt'] })
    expect(screen.queryByText(UNSUPPORTED_MAIN)).not.toBeInTheDocument()
  })

  it('flags .xlsx/.ppt/.rtf/odt like .docx', () => {
    const { unmount } = renderChatInput({ attachedFiles: ['/tmp/sheet.xlsx'] })
    expect(screen.getByText(UNSUPPORTED_MAIN)).toBeInTheDocument()
    unmount()
    renderChatInput({ attachedFiles: ['/tmp/deck.ppt'] })
    expect(screen.getByText(UNSUPPORTED_MAIN)).toBeInTheDocument()
  })

  it('shows for a mixed .txt + .docx set and auto-hides once the .docx is removed', () => {
    const { rerender } = renderChatInput({ attachedFiles: ['/tmp/notes.txt', '/tmp/report.docx'] })
    expect(screen.getByText(UNSUPPORTED_MAIN)).toBeInTheDocument()

    // Parent removes the .docx — the notice disappears with it, and the
    // dismissal state re-arms.
    rerender(
      <I18nProvider>
        <ChatInput
          value=""
          onChange={vi.fn()}
          onSend={vi.fn()}
          onExecuteSlash={vi.fn()}
          attachedFiles={['/tmp/notes.txt']}
          onAttach={vi.fn()}
          onDetachAll={vi.fn()}
          isQuerying={false}
          onCancelQuery={vi.fn()}
          onOpenQuickFix={vi.fn()}
          onOpenEditor={vi.fn()}
        />
      </I18nProvider>,
    )
    expect(screen.queryByText(UNSUPPORTED_MAIN)).not.toBeInTheDocument()
  })

  it('can be closed manually and re-arms for a newly attached unsupported file', () => {
    const { rerender } = renderChatInput({ attachedFiles: ['/tmp/report.docx'] })
    fireEvent.click(screen.getByRole('button', { name: 'Close' }))
    expect(screen.queryByText(UNSUPPORTED_MAIN)).not.toBeInTheDocument()

    // Removing the file resets dismissal...
    rerender(
      <I18nProvider>
        <ChatInput
          value=""
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
        />
      </I18nProvider>,
    )
    expect(screen.queryByText(UNSUPPORTED_MAIN)).not.toBeInTheDocument()

    // ...so the NEXT .docx warns again.
    rerender(
      <I18nProvider>
        <ChatInput
          value=""
          onChange={vi.fn()}
          onSend={vi.fn()}
          onExecuteSlash={vi.fn()}
          attachedFiles={['/tmp/other.docx']}
          onAttach={vi.fn()}
          onDetachAll={vi.fn()}
          isQuerying={false}
          onCancelQuery={vi.fn()}
          onOpenQuickFix={vi.fn()}
          onOpenEditor={vi.fn()}
        />
      </I18nProvider>,
    )
    expect(screen.getByText(UNSUPPORTED_MAIN)).toBeInTheDocument()
  })

  it('drag-drop still forwards office paths unchanged (send behavior untouched)', async () => {
    const onAttach = vi.fn()
    renderChatInput({ onAttach })
    await waitFor(() => expect(dragDrop.handler).toBeTruthy())
    act(() => {
      dragDrop.handler?.({ type: 'drop', paths: ['/tmp/report.docx'] } as WebviewFileDropEvent)
    })
    expect(onAttach).toHaveBeenCalledWith(['/tmp/report.docx'])
  })
})
