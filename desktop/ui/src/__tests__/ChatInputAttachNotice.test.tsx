// Office Wave 1 A1a, narrowed by G3 P1-5 — the composer must say out loud
// when an attachment's content will NOT be parsed (legacy .doc/.xls/…
// formats only; docx/xlsx/pptx ARE parsed backend-side and must not warn).
// The chip still attaches and the path is still sent; only the notice is
// new. See ChatInput.tsx UNPARSED_EXTENSIONS.
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor, act } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import ChatInput, { UNPARSED_EXTENSIONS, pathExtension } from '@/components/chat/ChatInput'
import type * as ReactRouterDom from 'react-router-dom'
import type { WebviewFileDropEvent } from '@/lib/tauri-api'

const UNSUPPORTED_MAIN = "This legacy format isn't parsed — its content was NOT sent to the model."
const UNSUPPORTED_HINT = 'Convert it to docx/xlsx/pptx/PDF or export the text, then attach that instead.'

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
    checkAttachmentPaths: vi.fn().mockResolvedValue([]),
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

  it('keeps only the truly-unparsed legacy formats flagged (G3 P1-5)', () => {
    // docx/xlsx/pptx ARE parsed backend-side (document_parse::OFFICE_EXTENSIONS
    // extracts their text) — flagging them was the banner lying.
    for (const ext of ['doc', 'xls', 'ppt', 'odt', 'rtf']) {
      expect(UNPARSED_EXTENSIONS.has(ext), ext).toBe(true)
    }
    // Parsed formats and everything else must NOT be flagged.
    for (const ext of ['docx', 'xlsx', 'pptx', 'ods', 'csv', 'pdf', 'txt', 'md', 'png']) {
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

  it('shows the notice when a legacy .doc is attached (chip still present)', () => {
    renderChatInput({ attachedFiles: ['/home/u/Downloads/report.doc'] })
    expect(screen.getByText('report.doc')).toBeInTheDocument() // chip unchanged
    expect(screen.getByText(UNSUPPORTED_MAIN)).toBeInTheDocument()
    expect(screen.getByText(UNSUPPORTED_HINT)).toBeInTheDocument()
  })

  it('does not show the notice for a plain .txt attachment', () => {
    renderChatInput({ attachedFiles: ['/tmp/notes.txt'] })
    expect(screen.queryByText(UNSUPPORTED_MAIN)).not.toBeInTheDocument()
  })

  it('does not lie about parsed formats: no notice for .docx/.xlsx/.pptx (G3 P1-5)', () => {
    for (const p of ['/tmp/report.docx', '/tmp/sheet.xlsx', '/tmp/deck.pptx']) {
      const { unmount } = renderChatInput({ attachedFiles: [p] })
      expect(screen.queryByText(UNSUPPORTED_MAIN)).not.toBeInTheDocument()
      unmount()
    }
  })

  it('flags .ppt/.odt/.rtf like .doc', () => {
    const { unmount } = renderChatInput({ attachedFiles: ['/tmp/deck.ppt'] })
    expect(screen.getByText(UNSUPPORTED_MAIN)).toBeInTheDocument()
    unmount()
    renderChatInput({ attachedFiles: ['/tmp/doc.odt'] })
    expect(screen.getByText(UNSUPPORTED_MAIN)).toBeInTheDocument()
  })

  it('shows for a mixed .txt + .doc set and auto-hides once the .doc is removed', () => {
    const { rerender } = renderChatInput({ attachedFiles: ['/tmp/notes.txt', '/tmp/report.doc'] })
    expect(screen.getByText(UNSUPPORTED_MAIN)).toBeInTheDocument()

    // Parent removes the .doc — the notice disappears with it, and the
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
    const { rerender } = renderChatInput({ attachedFiles: ['/tmp/report.doc'] })
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

    // ...so the NEXT .doc warns again.
    rerender(
      <I18nProvider>
        <ChatInput
          value=""
          onChange={vi.fn()}
          onSend={vi.fn()}
          onExecuteSlash={vi.fn()}
          attachedFiles={['/tmp/other.doc']}
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
