// L1 state machine — quickwin wave-2 (chat-testing plan v2 §9.3/§9.4).
//
// The v2 quick-win fixes live at the CHAT-PAGE layer: the composer's edit
// gating (A-25), the edit-mode attachment WYSIWYG contract (A-26) and the
// attachments-only queue chip + drain (D9-b pin) are behaviors of Chat.tsx
// bridging useChat() state and the composer context — the shared
// chatStateMachine.scripts.test.tsx pins AppContext event projections and
// deliberately does not grow page-level cases, so these land here (new
// file, shared-file discipline per briefs/_facts.md).
//
// Harness: same paradigm as Chat.test.tsx — the real Chat page over mocked
// chat/session/catalog contexts, so startEdit/commitEdit/handleSend and the
// queue-drain effect run for real against jsdom.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import * as dialog from '@tauri-apps/plugin-dialog'
import { I18nProvider } from '@/i18n'
import { ArtifactProvider } from '@/components/artifact/ArtifactContext'
import Chat from '@/pages/Chat'

const T0 = Date.UTC(2026, 0, 1, 10, 0)

const ctx = vi.hoisted(() => ({
  messages: [] as any[],
  streamingText: '',
  thinkingText: '',
  isQuerying: false,
  activeToolCalls: [] as any[],
  streamNotices: [] as any[],
  usage: null as any,
  sessions: [] as any[],
  currentSessionId: null as string | null,
  windowSessionId: null as string | null,
  error: null as string | null,
  errorKind: null as 'auth' | 'other' | null,
  providerStatus: null as any,
  config: null as any,
  feedback: {} as Record<string, string>,
  sendMessage: vi.fn().mockResolvedValue(true),
  cancelQuery: vi.fn(),
  checkpoints: [] as unknown[],
  rewindSession: vi.fn(),
  promptQueue: [] as any[],
  enqueuePrompt: vi.fn().mockReturnValue(true),
  dequeuePrompt: vi.fn().mockReturnValue(null),
  removeQueuedPrompt: vi.fn(),
  moveQueuedPrompt: vi.fn(),
  toolProgress: null as any,
  runProcess: null as any,
  sessionActivity: {} as Record<string, any>,
}))

vi.mock('@/context/ChatContext', () => ({
  useChat: () => ctx,
}))
// The real `SessionContext` export stays (useContext consumers) — only the
// hook is overridden (same shape as Chat.test.tsx).
vi.mock('@/context/SessionContext', async (importOriginal) => {
  const actual = await importOriginal<object>()
  return {
    ...actual,
    useSessions: () => ctx,
  }
})
vi.mock('@/context/CatalogContext', () => ({
  useCatalog: () => ctx,
}))

function resetCtx() {
  ctx.messages = []
  ctx.streamingText = ''
  ctx.thinkingText = ''
  ctx.isQuerying = false
  ctx.activeToolCalls = []
  ctx.streamNotices = []
  ctx.usage = null
  ctx.sessions = []
  ctx.currentSessionId = null
  ctx.windowSessionId = null
  ctx.error = null
  ctx.errorKind = null
  ctx.providerStatus = null
  ctx.config = null
  ctx.feedback = {}
  ctx.sendMessage = vi.fn().mockResolvedValue(true)
  ctx.cancelQuery = vi.fn()
  ctx.checkpoints = []
  ctx.rewindSession = vi.fn().mockResolvedValue([])
  ctx.promptQueue = []
  ctx.enqueuePrompt = vi.fn().mockReturnValue(true)
  ctx.dequeuePrompt = vi.fn().mockReturnValue(null)
  ctx.removeQueuedPrompt = vi.fn()
  ctx.moveQueuedPrompt = vi.fn()
  ctx.toolProgress = null
  ctx.runProcess = { status: 'idle', startedAt: null, endedAt: null, sources: [], outputs: [], summary: null, lastTool: null, toolCount: 0 }
  ctx.sessionActivity = {}
  localStorage.clear()
}

function ChatTree() {
  return (
    <I18nProvider>
      <MemoryRouter>
        <ArtifactProvider>
          <Chat />
        </ArtifactProvider>
      </MemoryRouter>
    </I18nProvider>
  )
}

function renderChat() {
  return render(<ChatTree />)
}

function att(path: string) {
  return { name: path.split('/').pop()!, path, size: 123 }
}

/** Seed: one user turn (with its attachments) + the assistant reply. */
function seedTurn(opts: { content?: string; attachments?: string[] } = {}) {
  ctx.messages = [
    {
      id: 'u1',
      role: 'user',
      content: opts.content ?? 'original question',
      timestamp: T0,
      ...(opts.attachments ? { file_attachments: opts.attachments.map(att) } : {}),
    },
    { id: 'a1', role: 'assistant', content: 'first answer', timestamp: T0 + 60_000 },
  ]
  ctx.checkpoints = [{ turn_index: 0 }]
  ctx.rewindSession = vi.fn().mockResolvedValue([])
}

async function attachViaMenu(path: string) {
  vi.mocked(dialog.open).mockResolvedValueOnce(path)
  fireEvent.click(screen.getByLabelText('Attachments and tools'))
  fireEvent.click(screen.getByRole('menuitem', { name: 'Attach file' }))
  await screen.findByText(path.split('/').pop()!)
}

function composerInput() {
  return screen.getByPlaceholderText(/Try: "Explain this repo"/)
}

/**
 * The composer chip for `path`, anchored on its remove button — the filename
 * alone is ambiguous (the conversation bubble's FileCard renders it too).
 */
function chipRemove(name: string) {
  return screen.queryByRole('button', { name: `Remove ${name}` })
}
function expectChip(name: string) {
  expect(chipRemove(name)).toBeInTheDocument()
}
function expectNoChip(name: string) {
  expect(chipRemove(name)).not.toBeInTheDocument()
}

async function startEdit() {
  fireEvent.click(screen.getByLabelText('Edit message'))
  await waitFor(() => expect(screen.getByTestId('edit-banner')).toBeInTheDocument())
}

beforeEach(() => {
  vi.clearAllMocks()
  resetCtx()
})

// ───────────────────────── A-26 — edit WYSIWYG ─────────────────────────

describe('A-26: edit-mode attachments are what-you-see-is-what-you-send', () => {
  it('startEdit loads the message’s own attachments into the composer as chips', async () => {
    seedTurn({ attachments: ['/Users/demo/workspace/my-startup/report-a.md', '/Users/demo/workspace/my-startup/notes-b.md'] })
    renderChat()
    // Pre-edit composer holds no chips (a stale draft carries nothing).
    expectNoChip('report-a.md')

    await startEdit()
    expect(composerInput()).toHaveValue('original question')
    // WYSIWYG: the ORIGINAL attachments render as removable composer chips —
    // they used to hide in editing.attachmentPaths while the composer showed
    // an unrelated draft set.
    expectChip('report-a.md')
    expectChip('notes-b.md')
  })

  it('commit sends the CURRENT composer set: remove one, add one, both honored', async () => {
    seedTurn({ attachments: ['/Users/demo/workspace/my-startup/report-a.md', '/Users/demo/workspace/my-startup/notes-b.md'] })
    renderChat()
    await startEdit()

    // Remove report-a.md via its chip ✕ …
    fireEvent.click(screen.getByRole('button', { name: 'Remove report-a.md' }))
    expectNoChip('report-a.md')
    // …and attach summary-c.md (the demo-harness path into mergePaths).
    await attachViaMenu('/Users/demo/workspace/my-startup/summary-c.md')

    fireEvent.change(composerInput(), { target: { value: 'edited question' } })
    fireEvent.keyDown(composerInput(), { key: 'Enter' })
    await waitFor(() => expect(ctx.rewindSession).toHaveBeenCalledWith(0))
    // The resend carries the EDITED set — not the message's original pair
    // (which the old editing.attachmentPaths commit silently resurrected).
    await waitFor(() =>
      expect(ctx.sendMessage).toHaveBeenCalledWith(
        'edited question',
        ['/Users/demo/workspace/my-startup/notes-b.md', '/Users/demo/workspace/my-startup/summary-c.md'],
      ),
    )
    expect(screen.queryByTestId('edit-banner')).not.toBeInTheDocument()
  })

  it('cancel restores the pre-edit draft: text AND the draft’s own attachments', async () => {
    seedTurn({ attachments: ['/Users/demo/workspace/my-startup/report-a.md'] })
    renderChat()
    // Park a draft (text + a chip) before entering edit mode.
    await attachViaMenu('/Users/demo/workspace/my-startup/draft-d.md')
    fireEvent.change(composerInput(), { target: { value: 'my precious draft' } })

    await startEdit()
    expect(composerInput()).toHaveValue('original question')
    expectChip('report-a.md')
    expectNoChip('draft-d.md')

    fireEvent.click(screen.getByLabelText('Cancel editing and restore draft'))
    // The draft mechanism survives the WYSIWYG swap-in: text + draft chip
    // return, the message's chips leave.
    expect(composerInput()).toHaveValue('my precious draft')
    expectChip('draft-d.md')
    expectNoChip('report-a.md')
  })

  it('regression: an attachment-less message still commits with zero attachments', async () => {
    seedTurn({})
    renderChat()
    await startEdit()
    fireEvent.change(composerInput(), { target: { value: 'edited question' } })
    fireEvent.keyDown(composerInput(), { key: 'Enter' })
    await waitFor(() => expect(ctx.sendMessage).toHaveBeenCalledWith('edited question', undefined))
    // The cleared composer must not regress either (post-commit reset).
    await waitFor(() => expect(composerInput()).toHaveValue(''))
  })

  it('regression: an untouched attachment set resends the original paths unchanged', async () => {
    seedTurn({ attachments: ['/Users/demo/workspace/my-startup/report-a.md', '/Users/demo/workspace/my-startup/notes-b.md'] })
    renderChat()
    await startEdit()
    fireEvent.change(composerInput(), { target: { value: 'edited question' } })
    fireEvent.keyDown(composerInput(), { key: 'Enter' })
    await waitFor(() =>
      expect(ctx.sendMessage).toHaveBeenCalledWith('edited question', [
        '/Users/demo/workspace/my-startup/report-a.md',
        '/Users/demo/workspace/my-startup/notes-b.md',
      ]),
    )
  })

  it('regression: a failed commit keeps the edited text AND the edited chips in the composer', async () => {
    seedTurn({ attachments: ['/Users/demo/workspace/my-startup/report-a.md'] })
    ctx.sendMessage = vi.fn().mockResolvedValue(false)
    renderChat()
    await startEdit()
    // Drop the original chip before committing — a failed send must NOT
    // resurrect it (the old code restored editing.attachmentPaths here).
    fireEvent.click(screen.getByRole('button', { name: 'Remove report-a.md' }))
    await attachViaMenu('/Users/demo/workspace/my-startup/summary-c.md')
    fireEvent.change(composerInput(), { target: { value: 'edited question' } })
    fireEvent.keyDown(composerInput(), { key: 'Enter' })
    await waitFor(() => expect(ctx.sendMessage).toHaveBeenCalled())
    await waitFor(() => expect(composerInput()).toHaveValue('edited question'))
    expectChip('summary-c.md')
    expectNoChip('report-a.md')
  })
})
