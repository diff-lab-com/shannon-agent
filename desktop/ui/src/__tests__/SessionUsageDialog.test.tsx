// SessionUsageDialog — composer 一键弹出的会话用量弹框(2026-09 三项 UX 修复 #3)。
// 验证:
//   - 有会话:六类 breakdown + 预算摘要行("已花费 / 上限") + 预算编辑入口(BudgetDialog 文案复用)。
//   - "查看完整用量"按钮:先关弹框再跳到 /usage。
//   - 无会话:显示空态,跳转按钮禁用。

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { I18nProvider } from '@/i18n'
import SessionUsageDialog from '@/components/chat/SessionUsageDialog'
import * as api from '@/lib/tauri-api'
import type { ContextBreakdown } from '@/types'

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

// 可变句柄:无会话用例在渲染前置 null。
const sessionCtx = vi.hoisted(() => ({ currentSessionId: 'sess-1' as string | null }))
vi.mock('@/context/SessionContext', () => ({
  useSessions: () => sessionCtx,
}))

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

describe('SessionUsageDialog', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(api.getSessionContextBreakdown).mockResolvedValue(breakdown)
    vi.mocked(api.getSessionUsage).mockResolvedValue({
      input_tokens: 600, output_tokens: 400, cache_creation_tokens: 0,
      cache_read_tokens: 300, cost_usd: 0.0123, events: 4,
    } as api.SessionUsageSummary)
    vi.mocked(api.getSessionBudget).mockResolvedValue(5)
  })

  it('renders the six-category breakdown and budget line', async () => {
    render(<SessionUsageDialog open onClose={() => {}} />, { wrapper })
    await waitFor(() => expect(screen.getByText('System prompt')).toBeInTheDocument())
    // 预算摘要行:已花费 / 上限
    expect(screen.getByText(/\$0\.0123 \/ \$5\.00/)).toBeInTheDocument()
    // 设置预算入口(复用 BudgetDialog 文案)
    expect(screen.getByRole('button', { name: /set session budget/i })).toBeInTheDocument()
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
