// SessionUsageDialog — composer 一键弹出的会话用量弹框(2026-09 三项 UX 修复 #3)。
// 验证:
//   - 有会话:六类 breakdown + 峰值行(设计稿 02c S3)+ 预算摘要行
//     ("花费 / 预算余",W7 口径)+ 预算编辑入口(BudgetDialog 文案复用)。
//   - "查看完整用量"按钮:先关弹框再跳到 /usage。
//   - 无会话:显示空态,跳转按钮禁用。
//   - 诚实契约:SessionInfo.max_context_tokens 缺席时峰值行整行隐藏(W8)。

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { I18nProvider } from '@/i18n'
import SessionUsageDialog from '@/components/chat/SessionUsageDialog'
import * as api from '@/lib/tauri-api'
import type { ContextBreakdown, SessionInfo } from '@/types'

vi.mock('@/lib/tauri-api', async () => {
  const actual = await vi.importActual<object>('@/lib/tauri-api')
  return {
    ...actual,
    getSessionContextBreakdown: vi.fn(),
    getSessionUsage: vi.fn(),
    getSessionBudget: vi.fn(),
    setSessionBudget: vi.fn(),
  }
})
// 全局 setup.ts 已 mock @tauri-apps/api/event(useSessionBudget 的 listen)。

// 可变句柄:无会话用例在渲染前置 null;峰值用例替换 sessions。
const sessionCtx = vi.hoisted(() => ({
  currentSessionId: 'sess-1' as string | null,
  sessions: [] as { id: string; title: string; created_at: number; message_count: number; max_context_tokens?: number }[],
}))
// The real `SessionContext` export stays (ContextBreakdownCard consumes it
// via useContext) — only the hook is overridden.
vi.mock('@/context/SessionContext', async (importOriginal) => {
  const actual = await importOriginal<object>()
  return {
    ...actual,
    useSessions: () => sessionCtx,
  }
})

const wrapper = ({ children }: { children: React.ReactNode }) => (
  <I18nProvider>
    <MemoryRouter initialEntries={['/chat']}>{children}</MemoryRouter>
  </I18nProvider>
)

const breakdown: ContextBreakdown = {
  totalTokens: 1000,
  contextWindow: 200000,
  categories: [
    { key: 'system', tokens: 400 },
    { key: 'tools', tokens: 250 },
    { key: 'skills', tokens: 50 },
    { key: 'memory', tokens: 50 },
    { key: 'mcp', tokens: 50 },
    { key: 'conversation', tokens: 200 },
  ],
}

const peakSession: SessionInfo = {
  id: 'sess-1',
  title: 'Fix billing webhook',
  created_at: 0,
  message_count: 1,
  max_context_tokens: 96000,
}

describe('SessionUsageDialog', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    sessionCtx.currentSessionId = 'sess-1'
    sessionCtx.sessions = [peakSession]
    vi.mocked(api.getSessionContextBreakdown).mockResolvedValue(breakdown)
    vi.mocked(api.getSessionUsage).mockResolvedValue({
      input_tokens: 600, output_tokens: 400, cache_creation_tokens: 0,
      cache_read_tokens: 300, cost_usd: 0.0123, events: 4,
    } as api.SessionUsageSummary)
    vi.mocked(api.getSessionBudget).mockResolvedValue(5)
  })

  it('renders the six-category breakdown and the budget-remaining line', async () => {
    render(<SessionUsageDialog open onClose={() => {}} />, { wrapper })
    await waitFor(() => expect(screen.getByText('System prompt')).toBeInTheDocument())
    // W7 (设计稿 02c S3): 预算行口径 = 花费 / 预算余(上限 − 已花)
    expect(screen.getByText(/\$0\.0123 \/ \$4\.9877/)).toBeInTheDocument()
    expect(screen.getByText(/left in budget/)).toBeInTheDocument()
    // 设置预算入口(复用 BudgetDialog 文案)
    expect(screen.getByRole('button', { name: /set session budget/i })).toBeInTheDocument()
  })

  // W8 (设计稿 02c S3「窗口峰值 96k」): the row reads SessionInfo
  // .max_context_tokens — the session-index peak; compact formatting like
  // the rail chip (96000 → "96K" in en).
  it('renders the session peak row from the session index', async () => {
    render(<SessionUsageDialog open onClose={() => {}} />, { wrapper })
    const peak = await screen.findByTestId('session-usage-peak')
    expect(peak).toHaveTextContent('Session peak')
    expect(peak).toHaveTextContent('96K')
  })

  // 诚实契约:数字缺席即隐藏 — no max_context_tokens on the session → the
  // whole row is gone, never a guessed window.
  it('hides the peak row when the session has no recorded peak', async () => {
    sessionCtx.sessions = [{ id: 'sess-1', title: 'Fix billing webhook', created_at: 0, message_count: 1 }]
    render(<SessionUsageDialog open onClose={() => {}} />, { wrapper })
    await waitFor(() => expect(screen.getByText('System prompt')).toBeInTheDocument())
    expect(screen.queryByTestId('session-usage-peak')).not.toBeInTheDocument()
  })

  // T2 (R3 §4.1 峰值已用): with BOTH the used numerator (SessionInfo
  // .peak_context_used_tokens) and the window budget present, the row
  // renders the % form 峰值 X / 预算 Y（Z%） — 12345/96000 → 13%, compact
  // tokens like the rest of the dialog ("12K / 96K (13%)").
  it('renders the % form when peak-used AND the window budget are both present', async () => {
    sessionCtx.sessions = [{
      id: 'sess-1', title: 'Fix billing webhook', created_at: 0, message_count: 1,
      max_context_tokens: 96_000, peak_context_used_tokens: 12_345,
    }]
    render(<SessionUsageDialog open onClose={() => {}} />, { wrapper })
    const peak = await screen.findByTestId('session-usage-peak')
    expect(peak).toHaveTextContent('Session peak')
    expect(peak).toHaveTextContent('12.3K / 96K (13%)')
  })

  // The % never fabricates headroom: used beyond the window clamps at 100%.
  it('clamps the peak percentage at 100', async () => {
    sessionCtx.sessions = [{
      id: 'sess-1', title: 'Fix billing webhook', created_at: 0, message_count: 1,
      max_context_tokens: 96_000, peak_context_used_tokens: 150_000,
    }]
    render(<SessionUsageDialog open onClose={() => {}} />, { wrapper })
    const peak = await screen.findByTestId('session-usage-peak')
    expect(peak).toHaveTextContent('150K / 96K (100%)')
  })

  // 反例: a used numerator WITHOUT a window budget can never produce a
  // percentage — and without the window there is no absolute form either,
  // so the whole row hides (数字缺席即隐藏).
  it('hides the peak row when only the used numerator exists', async () => {
    sessionCtx.sessions = [{
      id: 'sess-1', title: 'Fix billing webhook', created_at: 0, message_count: 1,
      peak_context_used_tokens: 12_345,
    }]
    render(<SessionUsageDialog open onClose={() => {}} />, { wrapper })
    await waitFor(() => expect(screen.getByText('System prompt')).toBeInTheDocument())
    expect(screen.queryByTestId('session-usage-peak')).not.toBeInTheDocument()
  })

  // 反例: a zero used figure is "no data" (never published by the backend
  // fold, never rendered) — the row keeps the plain absolute window form.
  it('keeps the absolute form when the used figure is zero', async () => {
    sessionCtx.sessions = [{
      id: 'sess-1', title: 'Fix billing webhook', created_at: 0, message_count: 1,
      max_context_tokens: 96_000, peak_context_used_tokens: 0,
    }]
    render(<SessionUsageDialog open onClose={() => {}} />, { wrapper })
    const peak = await screen.findByTestId('session-usage-peak')
    expect(peak).toHaveTextContent('Session peak')
    expect(peak).toHaveTextContent('96K')
    expect(peak).not.toHaveTextContent('(')
  })

  it('links to the full usage page and closes first', async () => {
    const onClose = vi.fn()
    render(<SessionUsageDialog open onClose={onClose} />, { wrapper })
    const viewAll = await screen.findByRole('button', { name: /view full usage/i })
    fireEvent.click(viewAll)
    expect(onClose).toHaveBeenCalled()
  })

  it('shows the empty state without a session', async () => {
    sessionCtx.currentSessionId = null
    render(<SessionUsageDialog open onClose={() => {}} />, { wrapper })
    expect(screen.getByText(/no active session/i)).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /view full usage/i })).toBeDisabled()
    sessionCtx.currentSessionId = 'sess-1' // 还原,避免影响其他用例
  })
})
