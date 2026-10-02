// R3 §4.2 L1 — 输入缓存锚点的状态机/页面层（配对 e2e/chat-input-persistence.spec.ts）。
//
// 草稿逻辑住在 Chat.tsx（localStorage shannon.draft.<id> + 300ms 防抖 +
// 切换同步 flush），不经过 ScriptPlayer，所以这一层沿用 Chat.test.tsx 的
// 页面级范式（mock 上下文 + rerender 切会话）：
//   - 切换 flush 竞态：防抖窗口内切走，旧会话草稿必须已落盘（R2-W1 丢稿
//     修复的回归锚点，finding: composer-draft）；
//   - 发送清空草稿；
//   - >64KB 仅内存 + 一次性 warn/toast 提示（A-21 已修复：仍不落盘，
//     但不再静默——console.warn 每次提示、toast 每挂载一次）。
// 队列（A-20 重启丢失）是 AppContext 内存态，重启语义只能在浏览器层锚
// ——见 e2e spec；drain/cap/排序已由 journey #9 双层覆盖。
//
// A-22（输入历史回溯不存在）：产品 gap，不建测试（仅此引用）。
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, act } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { I18nProvider } from '@/i18n'
import { ArtifactProvider } from '@/components/artifact/ArtifactContext'
import Chat from '@/pages/Chat'

const { toastMock } = vi.hoisted(() => ({
  toastMock: { error: vi.fn(), warning: vi.fn(), info: vi.fn(), message: vi.fn() },
}))
vi.mock('sonner', () => ({ toast: toastMock }))

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
  status: null as any,
  sendMessage: vi.fn().mockResolvedValue(true),
  cancelQuery: vi.fn(),
  checkpoints: [] as unknown[],
  rewindSession: vi.fn(),
  feedback: {} as Record<string, string>,
  recordFeedback: vi.fn().mockResolvedValue(undefined),
  createSession: vi.fn(),
  switchSession: vi.fn(),
  deleteSession: vi.fn(),
  renameSession: vi.fn(),
  promptQueue: [] as any[],
  enqueuePrompt: vi.fn().mockReturnValue(true),
  dequeuePrompt: vi.fn().mockReturnValue(null),
  removeQueuedPrompt: vi.fn(),
  moveQueuedPrompt: vi.fn(),
  toolProgress: null as any,
  runProcess: null as any,
  sessionActivity: {} as Record<string, any>,
}))

vi.mock('@/context/ChatContext', () => ({ useChat: () => ctx }))
vi.mock('@/context/SessionContext', async (importOriginal) => {
  const actual = await importOriginal<object>()
  return { ...actual, useSessions: () => ctx }
})
vi.mock('@/context/CatalogContext', () => ({ useCatalog: () => ctx }))

const SESSION_A = 'draft-sess-a'
const SESSION_B = 'draft-sess-b'
const keyOf = (id: string) => `shannon.draft.${id}`

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

function composer() {
  return screen.getByRole('textbox', { name: 'Message' }) as HTMLTextAreaElement
}

/** A FRESH element per render — identical elements bail out of re-renders,
 *  and the draft effects key on the (mocked) visible session id change. */
function renderOn(sessionId: string) {
  ctx.currentSessionId = sessionId
  return render(<ChatTree />)
}

async function switchTo(renderResult: ReturnType<typeof render>, sessionId: string) {
  ctx.currentSessionId = sessionId
  await act(async () => { renderResult.rerender(<ChatTree />) })
}

beforeEach(() => {
  vi.clearAllMocks()
  ctx.messages = []
  ctx.streamingText = ''
  ctx.isQuerying = false
  ctx.promptQueue = []
  ctx.windowSessionId = null
  ctx.error = null
  ctx.providerStatus = null
  ctx.config = null
  ctx.sessions = []
  ctx.toolProgress = null
  // RightDock consumes the run tab — the idle snapshot shape.
  ctx.runProcess = { status: 'idle', startedAt: null, endedAt: null, sources: [], outputs: [], summary: null, lastTool: null, toolCount: 0 }
  ctx.sessionActivity = {}
  localStorage.clear()
})

describe('input persistence L1 (§4.2) — per-session drafts (Chat.tsx)', () => {
  it('a switch INSIDE the 300ms debounce window flushes the old draft synchronously (R2-W1 anchor)', async () => {
    const view = renderOn(SESSION_A)
    fireEvent.change(composer(), { target: { value: 'A 的草稿' } })
    // No 300ms wait — the switch itself must persist the draft.
    await switchTo(view, SESSION_B)
    const flushed = localStorage.getItem(keyOf(SESSION_A))
    expect(flushed).toContain('A 的草稿')
    expect(JSON.parse(flushed!)).toMatchObject({ text: 'A 的草稿', attachments: [] })
    // B starts clean — no cross-contamination.
    expect(localStorage.getItem(keyOf(SESSION_B))).toBeNull()
    expect(composer().value).toBe('')
  })

  it('the composer restores the incoming session\'s draft on switch (round trip)', async () => {
    localStorage.setItem(keyOf(SESSION_B), JSON.stringify({ text: 'B 的草稿', attachments: [], updatedAt: Date.now() }))
    const view = renderOn(SESSION_A)
    await switchTo(view, SESSION_B)
    expect(composer().value).toBe('B 的草稿')
    await switchTo(view, SESSION_A)
    expect(composer().value).toBe('')
  })

  it('a manual send clears the draft key', async () => {
    renderOn(SESSION_A)
    fireEvent.change(composer(), { target: { value: '发出去的一条' } })
    await act(async () => { fireEvent.keyDown(composer(), { key: 'Enter' }) })
    expect(ctx.sendMessage).toHaveBeenCalledWith('发出去的一条', undefined)
    expect(composer().value).toBe('')
    // handleSend clears synchronously — the debounce never gets a say.
    expect(localStorage.getItem(keyOf(SESSION_A))).toBeNull()
  })

  // A-21 flipped: an oversized draft is still not persisted (the 64KB cap
  // protects the localStorage quota; the not-persisted behavior stays pinned
  // here and by the e2e restart anchor), but the skip is no longer SILENT —
  // every skipped write console.warns and the user gets exactly one toast
  // per mount. The input itself is never blocked: the draft keeps living in
  // the composer.
  it('drafts over the 64KB payload cap stay memory-only and warn once via toast (A-21 fixed)', async () => {
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})
    try {
      renderOn(SESSION_A)
      fireEvent.change(composer(), { target: { value: '大'.repeat(40_000) + '字'.repeat(30_000) } })
      // Past the debounce: the write ran — and skipped the oversized payload.
      await act(async () => { await new Promise(r => setTimeout(r, 400)) })
      expect(localStorage.getItem(keyOf(SESSION_A))).toBeNull()
      // The in-memory composer still holds it (no data loss while mounted).
      expect(composer().value.length).toBe(70_000)
      // A-21: the skip surfaces — one warn per skipped write, one toast.
      expect(warnSpy).toHaveBeenCalledTimes(1)
      expect(toastMock.warning).toHaveBeenCalledTimes(1)

      // Another oversized debounced write warns again but never re-toasts.
      fireEvent.change(composer(), { target: { value: '大'.repeat(40_000) + '字'.repeat(30_001) } })
      await act(async () => { await new Promise(r => setTimeout(r, 400)) })
      expect(composer().value.length).toBe(70_001)
      expect(warnSpy).toHaveBeenCalledTimes(2)
      expect(toastMock.warning).toHaveBeenCalledTimes(1)
    } finally {
      warnSpy.mockRestore()
    }
  })
})
