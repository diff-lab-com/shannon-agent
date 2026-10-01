// G3b P1-4 — extraction summaries from the `check_attachment_paths`
// preflight render as a chip badge whose tooltip says what the model will
// actually receive. Office sections vs PDF truncation wording; a parse
// failure (extracted: false) stays unbadged; pruned chips lose the badge.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor, act } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import ChatInput from '@/components/chat/ChatInput'
import type * as ReactRouterDom from 'react-router-dom'
import type { WebviewFileDropEvent } from '@/lib/tauri-api'
import * as api from '@/lib/tauri-api'
import type { AttachmentExtractionReport } from '@/types'

const { dragDrop, navigate } = vi.hoisted(() => ({
  dragDrop: { handler: null as null | ((e: unknown) => void) },
  navigate: vi.fn(),
}))

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

vi.mock('@/lib/tauri-api', async () => {
  const actual = await vi.importActual<object>('@/lib/tauri-api')
  return {
    ...actual,
    checkAttachmentPaths: vi.fn().mockResolvedValue([]),
    onWebviewFileDrop: vi.fn((handler: (e: unknown) => void) => {
      dragDrop.handler = handler
      return Promise.resolve(() => { dragDrop.handler = null })
    }),
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
  return render(<ChatInput {...defaultProps} {...props} />, { wrapper: I18nProvider })
}

async function dropPaths(paths: string[], onAttach: ReturnType<typeof vi.fn>) {
  await waitFor(() => expect(dragDrop.handler).toBeTruthy())
  act(() => {
    dragDrop.handler?.({ type: 'drop', paths } as WebviewFileDropEvent)
  })
  await waitFor(() => expect(onAttach).toHaveBeenCalledWith(paths))
}

const officeReport: AttachmentExtractionReport = {
  path: '/tmp/project/report.docx',
  kind: 'docx',
  extracted: true,
  sections_total: 23,
  sections_inlined: 8,
  truncated: true,
  cache_path: '/home/u/.shannon/cache/extracted/abc.txt',
}

const pdfReport: AttachmentExtractionReport = {
  path: '/tmp/project/manual.pdf',
  kind: 'pdf',
  extracted: true,
  sections_total: 0,
  sections_inlined: 0,
  truncated: true,
}

describe('ChatInput — extraction chip badges (G3b P1-4)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(api.checkAttachmentPaths).mockResolvedValue([])
    dragDrop.handler = null
  })

  it('badges an office chip with the extracted/inlined counts in the tooltip', async () => {
    vi.mocked(api.checkAttachmentPaths).mockResolvedValue([
      { path: '/tmp/project/report.docx', ok: true, extraction: officeReport },
    ])
    const onAttach = vi.fn()
    renderChatInput({ attachedFiles: ['/tmp/project/report.docx'], onAttach })
    await dropPaths(['/tmp/project/report.docx'], onAttach)
    const badge = await screen.findByTestId('attachment-chip-extraction')
    expect(badge).toHaveAttribute(
      'title',
      expect.stringContaining('Extracted 23 sections, first 8 inlined'),
    )
    expect(badge).toHaveAttribute('aria-label', expect.stringContaining('Extracted 23 sections'))
  })

  it('badges a truncated PDF chip with the 50 KiB / continue-reading wording', async () => {
    vi.mocked(api.checkAttachmentPaths).mockResolvedValue([
      { path: '/tmp/project/manual.pdf', ok: true, extraction: pdfReport },
    ])
    const onAttach = vi.fn()
    renderChatInput({ attachedFiles: ['/tmp/project/manual.pdf'], onAttach })
    await dropPaths(['/tmp/project/manual.pdf'], onAttach)
    const badge = await screen.findByTestId('attachment-chip-extraction')
    expect(badge).toHaveAttribute(
      'title',
      expect.stringContaining('PDF inlines the first 50 KiB'),
    )
  })

  it('a failed extraction and a plain ok path carry no badge', async () => {
    vi.mocked(api.checkAttachmentPaths).mockResolvedValue([
      {
        path: '/tmp/project/broken.docx',
        ok: true,
        extraction: { ...officeReport, path: '/tmp/project/broken.docx', extracted: false, sections_total: 0, sections_inlined: 0 },
      },
      { path: '/tmp/project/notes.txt', ok: true },
    ])
    const onAttach = vi.fn()
    renderChatInput({
      attachedFiles: ['/tmp/project/broken.docx', '/tmp/project/notes.txt'],
      onAttach,
    })
    await dropPaths(['/tmp/project/broken.docx', '/tmp/project/notes.txt'], onAttach)
    await waitFor(() => expect(api.checkAttachmentPaths).toHaveBeenCalled())
    expect(screen.getByText('broken.docx')).toBeInTheDocument()
    expect(screen.queryByTestId('attachment-chip-extraction')).not.toBeInTheDocument()
  })

  it('the badge is pruned when the chip is removed', async () => {
    vi.mocked(api.checkAttachmentPaths).mockResolvedValue([
      { path: '/tmp/project/report.docx', ok: true, extraction: officeReport },
    ])
    const onAttach = vi.fn()
    const { rerender } = renderChatInput({ attachedFiles: ['/tmp/project/report.docx'], onAttach })
    await dropPaths(['/tmp/project/report.docx'], onAttach)
    await screen.findByTestId('attachment-chip-extraction')
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
    await waitFor(() => expect(screen.queryByTestId('attachment-chip-extraction')).not.toBeInTheDocument())
  })
})
