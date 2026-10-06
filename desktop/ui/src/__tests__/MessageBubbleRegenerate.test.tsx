// B1-6 P2-1 — regenerate must honor sendMessage's boolean: the backend can
// refuse the resend (budget / concurrent-query / goal guards resolve FALSE,
// never throw), and the old unconditional success toast claimed a
// regeneration had started when nothing did. Pins all three branches:
//   * accepted → the success toast (existing behavior);
//   * rejected (resolves false) → NO success toast, no error throw — the
//     error surface is sendMessage's own banner (setChatError);
//   * rewind failure → the existing toastError catch stays.
//
// Harness mirrors MessageBubbleFileChanges.test.tsx (mocked contexts).

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, act } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { I18nProvider } from '@/i18n'

import { MessageBubble } from '@/components/chat/MessageBubble'
import { ArtifactProvider } from '@/components/artifact/ArtifactContext'
import type { ChatMessage } from '@/types'

const { toastMock } = vi.hoisted(() => ({
  toastMock: { error: vi.fn(), warning: vi.fn(), info: vi.fn(), success: vi.fn(), message: vi.fn() },
}))
vi.mock('sonner', () => ({ toast: toastMock }))

const ctx = vi.hoisted(() => ({
  messages: [] as any[],
  sendMessage: vi.fn().mockResolvedValue(true),
  feedback: {} as Record<string, string>,
  recordFeedback: vi.fn().mockResolvedValue(undefined),
  isQuerying: false,
  currentSessionId: 'session-1' as string | null,
  switchSession: vi.fn(),
  refreshSessions: vi.fn().mockResolvedValue(undefined),
  subagentLive: null as any,
  config: null as any,
}))

vi.mock('@/context/ChatContext', () => ({ useChat: () => ctx }))
vi.mock('@/context/SessionContext', async (importOriginal) => {
  const actual = await importOriginal<object>()
  return { ...actual, useSessions: () => ctx }
})
vi.mock('@/context/CatalogContext', () => ({ useCatalog: () => ctx }))

const REGENERATE = { turnIndex: 1, content: 'the original question', attachmentPaths: ['/tmp/a.txt'] }

function bubbleWith(message: ChatMessage, onRewind: (turnIndex: number) => Promise<void>) {
  return render(
    <I18nProvider>
      <MemoryRouter>
        <ArtifactProvider>
          <MessageBubble
            message={message}
            messageIndex={1}
            onViewDiff={() => {}}
            regenerate={REGENERATE}
            onRewind={onRewind}
          />
        </ArtifactProvider>
      </MemoryRouter>
    </I18nProvider>,
  )
}

const assistantMessage: ChatMessage = {
  role: 'assistant',
  content: 'the reply being regenerated',
  timestamp: 2,
}

function regenerateButton() {
  return screen.getByRole('button', { name: 'Regenerate response' })
}

beforeEach(() => {
  vi.clearAllMocks()
  ctx.sendMessage.mockReset()
  ctx.sendMessage.mockResolvedValue(true)
})

/** Drain the handleRegenerate await chain (rewind → sendMessage → toast):
 *  every promise in it is already settled, so full microtask drains inside
 *  act are deterministic — no timers, no real async. */
async function settleHandler() {
  await act(async () => {})
  await act(async () => {})
}

describe('B1-6 P2-1 — handleRegenerate vs sendMessage\'s result', () => {
  it('accepted resend still toasts success', async () => {
    const onRewind = vi.fn().mockResolvedValue(undefined)
    bubbleWith(assistantMessage, onRewind)

    fireEvent.click(regenerateButton())
    await settleHandler()

    expect(onRewind).toHaveBeenCalledWith(1)
    // Attachment paths travel on the resend (attachments-only payload).
    expect(ctx.sendMessage).toHaveBeenCalledWith('the original question', ['/tmp/a.txt'])
    expect(toastMock.success).toHaveBeenCalledWith('Rewound — regenerating the previous reply…')
  })

  it('rejected resend (resolves false) toasts NOTHING — the banner owns the error', async () => {
    const onRewind = vi.fn().mockResolvedValue(undefined)
    bubbleWith(assistantMessage, onRewind)

    // Budget pre-turn guard: sendMessage RESOLVES false, it never throws.
    ctx.sendMessage.mockResolvedValue(false)

    fireEvent.click(regenerateButton())
    await settleHandler()

    expect(onRewind).toHaveBeenCalledWith(1)
    expect(ctx.sendMessage).toHaveBeenCalled()
    // P2-1: no success claim for a send that never started — and no error
    // toast either (sendMessage already surfaced the rejection via the
    // error banner; the catch is for rewind failures only).
    expect(toastMock.success).not.toHaveBeenCalled()
    expect(toastMock.error).not.toHaveBeenCalled()
  })

  it('a failed rewind keeps the existing toastError catch', async () => {
    const onRewind = vi.fn().mockRejectedValue(new Error('no checkpoint'))
    bubbleWith(assistantMessage, onRewind)

    fireEvent.click(regenerateButton())
    await settleHandler()

    expect(ctx.sendMessage).not.toHaveBeenCalled()
    expect(toastMock.error).toHaveBeenCalledWith(
      "Couldn't regenerate the reply",
      { description: 'no checkpoint' },
    )
    expect(toastMock.success).not.toHaveBeenCalled()
  })
})
