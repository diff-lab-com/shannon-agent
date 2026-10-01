// GB P2-10b — the composer's @ file-reference popover.
//
// Typing `@` (start of input or after whitespace) opens the fuzzy file
// menu over the working-dir tree + file index; Enter/click inserts the
// path as plain text; Escape dismisses until the query changes.

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import React from 'react'
import { render, screen, fireEvent, waitFor, within } from '@testing-library/react'
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

vi.mock('@/context/CatalogContext', () => ({
  useCatalog: () => ({
    config: { approval_mode: 'suggest', model: 'm1', provider: 'anthropic', working_dir: '/w' },
    status: { model: 'M1', provider: 'anthropic', querying: false, message_count: 0, working_dir: '/w' },
    models: [{ id: 'm1', name: 'M1', provider: 'anthropic', context_window: 200000 }],
    refreshConfig: vi.fn(),
    refreshStatus: vi.fn(),
  }),
}))

vi.mock('@/context/SessionContext', () => ({
  useSessions: () => ({ currentSessionId: 'sess-1' }),
}))

const TREE = vi.hoisted(() => [
  {
    name: 'w', path: '/w', type: 'directory' as const,
    children: [
      { name: 'app.tsx', path: '/w/app.tsx', type: 'file' as const },
      { name: 'main.rs', path: '/w/main.rs', type: 'file' as const },
      { name: 'docs', path: '/w/docs', type: 'directory' as const, children: [
        { name: 'guide.md', path: '/w/docs/guide.md', type: 'file' as const },
      ] },
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
    listFileIndex: vi.fn().mockResolvedValue([
      { path: '/w/notes/summary.md', name: 'summary.md', size_bytes: 10, registered_at: '', favorite: false, source: 'attachment' },
    ]),
  }
})

type HostProps = Partial<React.ComponentProps<typeof ChatInput>>

/** Stateful host: mirrors what Chat.tsx does with the composer value so
 *  successive fireEvent.change calls behave like real keystrokes. */
function MentionHost(props: HostProps) {
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

function renderMentionHost(props: HostProps = {}) {
  return render(<MentionHost {...props} />, { wrapper: I18nProvider })
}

const textarea = () => screen.getByRole('textbox', { name: 'Message' })

describe('ChatInput @ file mentions (GB P2-10b)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  afterEach(() => {
    document.querySelectorAll('[role="listbox"]').forEach(n => n.remove())
  })

  it('loads candidates from the file tree + file index for the working dir', async () => {
    renderMentionHost()
    await waitFor(() => expect(api.getFileTree).toHaveBeenCalledWith('/w'))
    await waitFor(() => expect(api.listFileIndex).toHaveBeenCalled())
    // Typing '@' opens the menu with everything (empty query).
    fireEvent.change(textarea(), { target: { value: '@' } })
    const listbox = await screen.findByRole('listbox', { name: 'File mentions' })
    const options = within(listbox).queryAllByRole('option')
    expect(options.length).toBeGreaterThanOrEqual(4) // 3 tree files + 1 index entry
  })

  it('filters as the token grows and inserts the relative path on Enter', async () => {
    renderMentionHost()
    fireEvent.change(textarea(), { target: { value: 'check ' } })
    fireEvent.change(textarea(), { target: { value: 'check @gui' } })
    const listbox = await screen.findByRole('listbox', { name: 'File mentions' })
    const options = within(listbox).queryAllByRole('option')
    expect(options).toHaveLength(1)
    expect(options[0]).toHaveTextContent('docs/guide.md')

    fireEvent.keyDown(textarea(), { key: 'Enter' })
    expect(textarea()).toHaveValue('check @docs/guide.md ')
  })

  it('click-picking inserts plain text and never touches attachments', async () => {
    const onAttach = vi.fn()
    renderMentionHost({ onAttach })
    fireEvent.change(textarea(), { target: { value: '@main' } })
    const listbox = await screen.findByRole('listbox', { name: 'File mentions' })
    const option = within(listbox).getByRole('option', { name: /main\.rs/ })
    fireEvent.mouseDown(option)
    expect(textarea()).toHaveValue('@main.rs ')
    expect(onAttach).not.toHaveBeenCalled()
  })

  it('Escape dismisses; the menu re-arms once the query dissolves', async () => {
    renderMentionHost()
    fireEvent.change(textarea(), { target: { value: '@ma' } })
    await screen.findByRole('listbox', { name: 'File mentions' })
    fireEvent.keyDown(textarea(), { key: 'Escape' })
    expect(screen.queryByRole('listbox', { name: 'File mentions' })).not.toBeInTheDocument()
    // Extended token (query never dissolved) stays dismissed…
    fireEvent.change(textarea(), { target: { value: '@mai' } })
    expect(screen.queryByRole('listbox', { name: 'File mentions' })).not.toBeInTheDocument()
    // …but after the query dissolves (whitespace), a NEW @query opens fresh.
    fireEvent.change(textarea(), { target: { value: '@mai is done' } })
    fireEvent.change(textarea(), { target: { value: '@mai is done @gui' } })
    await screen.findByRole('listbox', { name: 'File mentions' })
  })

  it('mid-word @ (email) never opens the menu', async () => {
    renderMentionHost()
    await waitFor(() => expect(api.getFileTree).toHaveBeenCalled())
    fireEvent.change(textarea(), { target: { value: 'mail user@example.com' } })
    expect(screen.queryByRole('listbox', { name: 'File mentions' })).not.toBeInTheDocument()
  })

  it('degrades silently when both sources fail', async () => {
    vi.mocked(api.getFileTree).mockRejectedValueOnce(new Error('no wd'))
    vi.mocked(api.listFileIndex).mockRejectedValueOnce(new Error('no index'))
    renderMentionHost()
    await waitFor(() => expect(api.getFileTree).toHaveBeenCalled())
    fireEvent.change(textarea(), { target: { value: '@ma' } })
    expect(screen.queryByRole('listbox', { name: 'File mentions' })).not.toBeInTheDocument()
  })
})
