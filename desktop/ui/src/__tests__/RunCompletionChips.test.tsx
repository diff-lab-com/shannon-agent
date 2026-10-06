// RunCompletionChips (src/components/chat/RunCompletionChips.tsx) — D5 方案①
// post-completion suggestion rules, pinned at the component level:
//   failed → 重试 + 分析失败原因; success + changes → 提交这些改动;
//   otherwise nothing; suggestions.enabled = false → nothing; a new send
//   (runProcess reset to running) dismisses the chips. A chip FILLS the
//   composer via setInput and must never send.

import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import { ChatContext, type ChatContextValue } from '@/context/ChatContext'
import { CatalogContext, type CatalogContextValue } from '@/context/CatalogContext'
import { ComposerContext, type ComposerContextValue } from '@/pages/chat/ComposerContext'
import RunCompletionChips from '@/components/chat/RunCompletionChips'
import { beginRun, endRun, noteToolStart, type RunProcessState } from '@/lib/runProcess'
import type { ChatMessage } from '@/types'

// Real reducer states — the chips must agree with what the run lifecycle
// actually produces, not with hand-shaped objects.
const failedRun = (hadChanges = false): RunProcessState =>
  endRun(
    hadChanges
      ? noteToolStart(beginRun({ at: 1, message: 'fix the bug' }), 'edit', { file_path: '/w/a.ts' }, 2)
      : beginRun({ at: 1, message: 'fix the bug' }),
    9,
    true,
  )
const doneRun = (hadChanges: boolean): RunProcessState =>
  endRun(
    hadChanges
      ? noteToolStart(beginRun({ at: 1, message: 'add a feature' }), 'write', { path: '/w/b.ts' }, 2)
      : beginRun({ at: 1, message: 'add a feature' }),
    9,
    false,
  )

const USER_MSG: ChatMessage = { role: 'user', content: 'fix the login bug', timestamp: 1 }

function renderChips(overrides: {
  runProcess?: RunProcessState
  isQuerying?: boolean
  messages?: ChatMessage[]
  config?: Record<string, unknown> | null
  setInput?: (s: string) => void
} = {}) {
  const setInput = overrides.setInput ?? vi.fn()
  const chatValue = {
    messages: overrides.messages ?? [USER_MSG],
    streamingText: '',
    thinkingText: '',
    isQuerying: overrides.isQuerying ?? false,
    activeToolCalls: [],
    toolProgress: null,
    streamNotices: [],
    usage: null,
    runProcess: overrides.runProcess ?? { status: 'idle', startedAt: null, endedAt: null, sources: [], outputs: [], summary: null, lastTool: null, toolCount: 0, hadChanges: false },
    promptQueue: [],
    visionConfirm: null,
    toolsConfirm: null,
    checkpoints: [],
    feedback: {},
    contextPanelOpen: false,
  } as unknown as ChatContextValue
  const catalogValue = { config: overrides.config ?? null } as unknown as CatalogContextValue
  const composerValue = { setInput } as unknown as ComposerContextValue
  render(
    <I18nProvider>
      <CatalogContext.Provider value={catalogValue}>
        <ChatContext.Provider value={chatValue}>
          <ComposerContext.Provider value={composerValue}>
            <RunCompletionChips />
          </ComposerContext.Provider>
        </ChatContext.Provider>
      </CatalogContext.Provider>
    </I18nProvider>,
  )
  return { setInput }
}

describe('RunCompletionChips', () => {
  it('failed run → exactly two chips: 重试 + 分析失败原因', () => {
    renderChips({ runProcess: failedRun(false) })
    expect(screen.getByTestId('run-completion-chip-retry')).toBeInTheDocument()
    expect(screen.getByTestId('run-completion-chip-analyze-failure')).toBeInTheDocument()
    expect(screen.queryByTestId('run-completion-chip-commit-changes')).not.toBeInTheDocument()
  })

  it('failed run without a user message → only the analyze chip (nothing to retry)', () => {
    renderChips({ runProcess: failedRun(false), messages: [] })
    expect(screen.queryByTestId('run-completion-chip-retry')).not.toBeInTheDocument()
    expect(screen.getByTestId('run-completion-chip-analyze-failure')).toBeInTheDocument()
  })

  it('retry chip FILLS the composer with the last user message — never sends', () => {
    const setInput = vi.fn()
    renderChips({ runProcess: failedRun(false), setInput })
    fireEvent.click(screen.getByTestId('run-completion-chip-retry'))
    expect(setInput).toHaveBeenCalledTimes(1)
    expect(setInput).toHaveBeenCalledWith('fix the login bug')
  })

  it('analyze chip fills the composer with the preset analysis prompt', () => {
    const setInput = vi.fn()
    renderChips({ runProcess: failedRun(false), setInput })
    fireEvent.click(screen.getByTestId('run-completion-chip-analyze-failure'))
    expect(setInput).toHaveBeenCalledTimes(1)
    expect(setInput.mock.calls[0][0]).toMatch(/analyze why the last run failed/i)
  })

  it('succeeded run WITH file changes → exactly one chip: 提交这些改动', () => {
    renderChips({ runProcess: doneRun(true) })
    expect(screen.getByTestId('run-completion-chip-commit-changes')).toBeInTheDocument()
    expect(screen.queryByTestId('run-completion-chip-retry')).not.toBeInTheDocument()
    expect(screen.queryByTestId('run-completion-chip-analyze-failure')).not.toBeInTheDocument()
  })

  it('commit chip fills the composer with the commit-assistant prompt', () => {
    const setInput = vi.fn()
    renderChips({ runProcess: doneRun(true), setInput })
    fireEvent.click(screen.getByTestId('run-completion-chip-commit-changes'))
    expect(setInput).toHaveBeenCalledTimes(1)
    expect(setInput.mock.calls[0][0]).toMatch(/commit the changes from the last run/i)
  })

  it('succeeded run WITHOUT file changes → no chips (noise avoidance)', () => {
    renderChips({ runProcess: doneRun(false) })
    expect(screen.queryByTestId('run-completion-chips')).not.toBeInTheDocument()
    expect(screen.queryByTestId('run-completion-chip-commit-changes')).not.toBeInTheDocument()
  })

  it('idle (no run) → no chips', () => {
    renderChips({})
    expect(screen.queryByTestId('run-completion-chips')).not.toBeInTheDocument()
  })

  it('a still-running run → no chips', () => {
    renderChips({ runProcess: failedRun(true), isQuerying: true })
    expect(screen.queryByTestId('run-completion-chips')).not.toBeInTheDocument()
  })

  it('gate off (suggestions.enabled = false) → renders nothing even on a failed run', () => {
    renderChips({ runProcess: failedRun(true), config: { suggestions_enabled: false } })
    expect(screen.queryByTestId('run-completion-chips')).not.toBeInTheDocument()
  })

  it('gate on (explicit suggestions_enabled = true) → chips render', () => {
    renderChips({ runProcess: failedRun(false), config: { suggestions_enabled: true } })
    expect(screen.getByTestId('run-completion-chips')).toBeInTheDocument()
  })

  it('dismiss on send: a new run (runProcess reset to running) removes the chips', () => {
    const settled = doneRun(true)
    const { rerender } = render(
      <I18nProvider>
        <CatalogContext.Provider value={{} as CatalogContextValue}>
          <ChatContext.Provider value={{
            messages: [USER_MSG],
            isQuerying: false,
            runProcess: settled,
          } as unknown as ChatContextValue}>
            <ComposerContext.Provider value={{ setInput: vi.fn() } as unknown as ComposerContextValue}>
              <RunCompletionChips />
            </ComposerContext.Provider>
          </ChatContext.Provider>
        </CatalogContext.Provider>
      </I18nProvider>,
    )
    expect(screen.getByTestId('run-completion-chip-commit-changes')).toBeInTheDocument()
    // The user sends again — beginRun resets the snapshot to running.
    rerender(
      <I18nProvider>
        <CatalogContext.Provider value={{} as CatalogContextValue}>
          <ChatContext.Provider value={{
            messages: [USER_MSG],
            isQuerying: false,
            runProcess: beginRun({ at: 100, message: 'fix the bug' }),
          } as unknown as ChatContextValue}>
            <ComposerContext.Provider value={{ setInput: vi.fn() } as unknown as ComposerContextValue}>
              <RunCompletionChips />
            </ComposerContext.Provider>
          </ChatContext.Provider>
        </CatalogContext.Provider>
      </I18nProvider>,
    )
    expect(screen.queryByTestId('run-completion-chips')).not.toBeInTheDocument()
  })
})
