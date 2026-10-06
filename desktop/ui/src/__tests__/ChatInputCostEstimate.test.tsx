// S3-6 — pre-send cost estimate row: renders the backend's billing-grade
// projection (tokens + ≈low–high range) after the 300 ms debounce, hides on
// an empty composer / a failed call (advisory-only contract — never an error
// toast, never a send gate), and links to the session budget at ≥80% with a
// warning tint + share copy (BudgetBanner stays the single banner).
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, act } from '@testing-library/react'
import ChatInput, { ESTIMATE_DEBOUNCE_MS } from '@/components/chat/ChatInput'
import type * as ReactRouterDom from 'react-router-dom'

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

const mockRefreshConfig = vi.fn()
const mockRefreshStatus = vi.fn()

vi.mock('@/context/CatalogContext', () => ({
  useCatalog: () => ({
    config: { approval_mode: 'suggest', model: 'claude-sonnet-4-6', provider: 'anthropic' },
    status: { model: 'Claude Sonnet 4.6', provider: 'anthropic', querying: false, message_count: 0, working_dir: '/home/user/projects' },
    models: [
      { id: 'claude-sonnet-4-6', name: 'Claude Sonnet 4.6', provider: 'anthropic', context_window: 200000 },
    ],
    refreshConfig: mockRefreshConfig,
    refreshStatus: mockRefreshStatus,
  }),
}))

const estimateSendCost = vi.hoisted(() => ({ fn: vi.fn() }))
vi.mock('@/lib/tauri-api', async () => {
  const actual = await vi.importActual<object>('@/lib/tauri-api')
  return {
    ...actual,
    configure: vi.fn().mockResolvedValue(undefined),
    setSessionModel: vi.fn().mockResolvedValue(undefined),
    clearSessionModel: vi.fn().mockResolvedValue(undefined),
    getSessionModel: vi.fn().mockResolvedValue(null),
    estimateSendCost: estimateSendCost.fn,
    getSessionContextBreakdown: vi.fn().mockResolvedValue({
      totalTokens: 0, contextWindow: null,
      categories: [
        { key: 'system', tokens: 0 }, { key: 'tools', tokens: 0 }, { key: 'skills', tokens: 0 },
        { key: 'memory', tokens: 0 }, { key: 'mcp', tokens: 0 }, { key: 'conversation', tokens: 0 },
      ],
    }),
    getSessionUsage: vi.fn().mockResolvedValue({ cost_usd: 0 }),
    getSessionBudget: vi.fn().mockResolvedValue(null),
    onWebviewFileDrop: vi.fn().mockResolvedValue(() => {}),
  }
})

const BASE_ESTIMATE = {
  model: 'claude-sonnet-4-6',
  inputTokens: 1234,
  contextTokens: 1000,
  draftTokens: 234,
  attachmentTokens: 0,
  maxOutputTokens: 4096,
  costLow: 0.0037,
  costHigh: 0.0649,
  budgetUsd: null,
  spentUsd: 0,
}

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

beforeEach(() => {
  vi.useFakeTimers()
  estimateSendCost.fn.mockReset()
  estimateSendCost.fn.mockResolvedValue({ ...BASE_ESTIMATE })
})

afterEach(() => {
  vi.useRealTimers()
})

describe('ChatInput pre-send cost estimate (S3-6)', () => {
  it('stays hidden while the composer is empty', () => {
    renderChatInput()
    act(() => { vi.advanceTimersByTime(ESTIMATE_DEBOUNCE_MS * 2) })
    expect(screen.queryByTestId('send-cost-estimate')).not.toBeInTheDocument()
    expect(estimateSendCost.fn).not.toHaveBeenCalled()
  })

  it('renders tokens + the cost range after the debounce', async () => {
    const { rerender } = renderChatInput({ value: '' })
    rerender(<ChatInput value="hello world" onChange={vi.fn()} onSend={vi.fn()} onExecuteSlash={vi.fn()} attachedFiles={[]} onAttach={vi.fn()} onDetachAll={vi.fn()} isQuerying={false} onCancelQuery={vi.fn()} onOpenQuickFix={vi.fn()} onOpenEditor={vi.fn()} sessionId="sess-1" />)
    // Inside the debounce window: no call yet.
    act(() => { vi.advanceTimersByTime(ESTIMATE_DEBOUNCE_MS - 1) })
    expect(estimateSendCost.fn).not.toHaveBeenCalled()
    // Settled: exactly one call, carrying draft + session + attachments.
    act(() => { vi.advanceTimersByTime(1) })
    // Flush the resolved promise microtasks (fake timers don't run them).
    await act(async () => {})
    expect(estimateSendCost.fn).toHaveBeenCalledTimes(1)
    expect(estimateSendCost.fn).toHaveBeenCalledWith('sess-1', 'hello world', [])
    // Range copy: token count and both ends of the interval.
    const row = screen.getByTestId('send-cost-estimate')
    expect(row).toHaveTextContent(/1,234/)
    expect(row).toHaveTextContent(/\$0\.0037/)
    expect(row).toHaveTextContent(/\$0\.0649/)
  })

  it('debounces rapid keystrokes into one call', async () => {
    const view = renderChatInput({ value: '' })
    for (const text of ['h', 'he', 'hel', 'hell', 'hello']) {
      view.rerender(<ChatInput value={text} onChange={vi.fn()} onSend={vi.fn()} onExecuteSlash={vi.fn()} attachedFiles={[]} onAttach={vi.fn()} onDetachAll={vi.fn()} isQuerying={false} onCancelQuery={vi.fn()} onOpenQuickFix={vi.fn()} onOpenEditor={vi.fn()} sessionId="sess-1" />)
      act(() => { vi.advanceTimersByTime(100) })
    }
    // Four restarts — the 5th settles: 300ms more and exactly ONE call.
    act(() => { vi.advanceTimersByTime(ESTIMATE_DEBOUNCE_MS) })
    await act(async () => {})
    expect(estimateSendCost.fn).toHaveBeenCalledTimes(1)
    expect(estimateSendCost.fn).toHaveBeenCalledWith('sess-1', 'hello', [])
  })

  it('turns warning-toned and names the share at ≥80% of the session budget', async () => {
    estimateSendCost.fn.mockResolvedValue({
      ...BASE_ESTIMATE,
      budgetUsd: 0.08,
      spentUsd: 0.06,
      // spent 0.06 + high 0.0649 = 0.1249 / 0.08 → 156% — well past the mark.
    })
    renderChatInput({ value: 'hello' })
    act(() => { vi.advanceTimersByTime(ESTIMATE_DEBOUNCE_MS) })
    await act(async () => {})
    expect(screen.getByTestId('send-cost-budget-note')).toHaveTextContent(/1\d\d%|% of session budget|session budget/i)
    // Warning tone on the row itself.
    expect(screen.getByTestId('send-cost-estimate').className).toMatch(/warning/)
  })

  it('stays silent when the backend has no estimate (advisory contract)', async () => {
    estimateSendCost.fn.mockRejectedValue(new Error('no such command'))
    const { container } = renderChatInput({ value: 'hello' })
    act(() => { vi.advanceTimersByTime(ESTIMATE_DEBOUNCE_MS) })
    // Both the call and the rejected-promise catch need microtask turns.
    await act(async () => {})
    await act(async () => {})
    expect(estimateSendCost.fn).toHaveBeenCalled()
    expect(screen.queryByTestId('send-cost-estimate')).not.toBeInTheDocument()
    // No error banner, no toast — the failure is invisible by design.
    expect(container.textContent).not.toMatch(/failed/i)
  })
})
