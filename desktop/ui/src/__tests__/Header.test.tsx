import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router-dom'
import { I18nProvider } from '@/i18n'
import { Header } from '@/components/Header'
import * as api from '@/lib/tauri-api'

const mockCtx = vi.hoisted(() => ({
  status: { model: 'claude-sonnet-4-6', provider: 'anthropic', querying: false } as any,
  models: [
    { id: 'claude-sonnet-4-6', name: 'Claude Sonnet', provider: 'anthropic', context_window: 200000 },
    { id: 'gpt-4o', name: 'GPT-4o', provider: 'openai', context_window: 128000 },
  ],
  permissionRequest: null as any,
  respondPermission: vi.fn(),
  refreshConfig: vi.fn(),
  refreshStatus: vi.fn(),
}))

// U2: Header reads the session slice (title on /chat) and the chat slice
// (ContextPanel toggle state, owned by AppProvider).
const mockSessionCtx = vi.hoisted(() => ({
  sessions: [] as any[],
  currentSessionId: null as string | null,
}))
const mockChatCtx = vi.hoisted(() => ({
  contextPanelOpen: false,
  toggleContextPanel: vi.fn(),
}))

vi.mock('@/context/CatalogContext', () => ({
  useCatalog: () => mockCtx,
}))
vi.mock('@/context/SessionContext', () => ({
  useSessions: () => mockSessionCtx,
}))
vi.mock('@/context/ChatContext', () => ({
  useChat: () => mockChatCtx,
}))

function wrap(ui: React.ReactElement, { route = '/chat' } = {}) {
  return (
    <I18nProvider>
      <MemoryRouter initialEntries={[route]}>
        {ui}
      </MemoryRouter>
    </I18nProvider>
  )
}

describe('Header component', () => {
  beforeEach(() => {
    mockCtx.status = { model: 'claude-sonnet-4-6', provider: 'anthropic', querying: false }
    mockCtx.models = [
      { id: 'claude-sonnet-4-6', name: 'Claude Sonnet', provider: 'anthropic', context_window: 200000 },
      { id: 'gpt-4o', name: 'GPT-4o', provider: 'openai', context_window: 128000 },
    ]
    mockCtx.permissionRequest = null
    mockCtx.respondPermission = vi.fn()
    mockCtx.refreshConfig = vi.fn()
    mockCtx.refreshStatus = vi.fn()
    mockSessionCtx.sessions = []
    mockSessionCtx.currentSessionId = null
    mockChatCtx.contextPanelOpen = false
    mockChatCtx.toggleContextPanel = vi.fn()
  })

  it('renders page title based on route', () => {
    render(wrap(<Header />, { route: '/chat' }))
    expect(screen.getByText('Chat')).toBeInTheDocument()
  })

  // U2 — the global Header carries the current session's title on /chat
  // (replaces the retired per-page ChatHeader).
  it('shows the current session title on /chat', () => {
    mockSessionCtx.sessions = [
      { id: 's1', title: 'Q3 roadmap brainstorm', created_at: 1, message_count: 0 },
    ]
    mockSessionCtx.currentSessionId = 's1'
    render(wrap(<Header />, { route: '/chat' }))
    expect(screen.getByText('Q3 roadmap brainstorm')).toBeInTheDocument()
    expect(screen.queryByText('Chat')).not.toBeInTheDocument()
  })

  it('keeps the fixed Chat title when no session is active', () => {
    render(wrap(<Header />, { route: '/chat' }))
    expect(screen.getByText('Chat')).toBeInTheDocument()
  })

  it('keeps TITLE_MAP titles on non-chat pages even with a session active', () => {
    mockSessionCtx.sessions = [
      { id: 's1', title: 'Q3 roadmap brainstorm', created_at: 1, message_count: 0 },
    ]
    mockSessionCtx.currentSessionId = 's1'
    render(wrap(<Header />, { route: '/tasks' }))
    expect(screen.getByText('Scheduled')).toBeInTheDocument()
  })

  // U2 — ContextPanel toggle moved here from the retired ChatHeader.
  it('renders the ContextPanel toggle on /chat and wires it to the chat slice', () => {
    render(wrap(<Header />, { route: '/chat' }))
    const toggle = screen.getByRole('button', { name: 'Toggle context panel' })
    expect(toggle).toHaveAttribute('aria-pressed', 'false')
    fireEvent.click(toggle)
    expect(mockChatCtx.toggleContextPanel).toHaveBeenCalledTimes(1)
  })

  it('reflects open state on the ContextPanel toggle', () => {
    mockChatCtx.contextPanelOpen = true
    render(wrap(<Header />, { route: '/chat' }))
    expect(screen.getByRole('button', { name: 'Toggle context panel' })).toHaveAttribute('aria-pressed', 'true')
  })

  it('does not render the ContextPanel toggle on other pages', () => {
    render(wrap(<Header />, { route: '/tasks' }))
    expect(screen.queryByRole('button', { name: 'Toggle context panel' })).not.toBeInTheDocument()
  })

  it('renders model selector with current model name', () => {
    render(wrap(<Header />, { route: '/chat' }))
    expect(screen.getByText('claude-sonnet-4-6')).toBeInTheDocument()
  })

  it('opens model dropdown with model names on click', async () => {
    render(wrap(<Header />, { route: '/chat' }))
    fireEvent.click(screen.getByText('claude-sonnet-4-6'))
    await waitFor(() => {
      expect(screen.getByText('Claude Sonnet')).toBeInTheDocument()
      expect(screen.getByText('GPT-4o')).toBeInTheDocument()
    })
  })

  // U2 — Header absorbed ChatInput's dual-write: it configures the model
  // NAME plus the model's provider (not just the catalog id).
  it('switches model when option is clicked', async () => {
    const api = await import('@/lib/tauri-api')
    render(wrap(<Header />, { route: '/chat' }))
    fireEvent.click(screen.getByText('claude-sonnet-4-6'))
    await waitFor(() => {
      expect(screen.getByText('GPT-4o')).toBeInTheDocument()
    })
    fireEvent.click(screen.getByText('GPT-4o'))
    await waitFor(() => {
      expect(api.configure).toHaveBeenCalledWith({ key: 'model', value: 'GPT-4o' })
      expect(api.configure).toHaveBeenCalledWith({ key: 'provider', value: 'openai' })
    })
  })

  it('renders OPC title on /opc route', () => {
    render(wrap(<Header />, { route: '/opc' }))
    expect(screen.getByText('One Person Company')).toBeInTheDocument()
  })

  it('renders sync status badge on /opc/task route', () => {
    render(wrap(<Header />, { route: '/opc/task' }))
    expect(screen.getByText(/Sync Status/)).toBeInTheDocument()
  })

  it('renders user avatar placeholder', () => {
    render(wrap(<Header />, { route: '/chat' }))
    expect(screen.getByText('person')).toBeInTheDocument()
  })

  it('renders permission modal when permission request is present', () => {
    mockCtx.permissionRequest = { request_id: 'p1', tool: 'bash', risk: 'high', input: { cmd: 'rm -rf' } }
    render(wrap(<Header />, { route: '/chat' }))
    expect(screen.getByText('Permission Request')).toBeInTheDocument()
    expect(screen.getByText('bash')).toBeInTheDocument()
    expect(screen.getByText('Allow Once')).toBeInTheDocument()
    expect(screen.getByText('Deny')).toBeInTheDocument()
  })

  // U3 — four distinguishable risk tiers: critical=error, high=secondary,
  // medium=tertiary (was wrongly secondary), low=tertiary. Localized text,
  // announced via aria-label.
  describe.each(['critical', 'high', 'medium', 'low'] as const)('risk tier %s', (risk) => {
    const label = { critical: 'Critical', high: 'High', medium: 'Medium', low: 'Low' }[risk]
    const tier = { critical: 'text-error', high: 'text-secondary', medium: 'text-tertiary', low: 'text-tertiary' }[risk]

    it(`renders a localized "${label}" badge in the ${tier} tier`, () => {
      mockCtx.permissionRequest = { request_id: 'p1', tool: 'bash', risk, input: null }
      render(wrap(<Header />, { route: '/chat' }))
      const badge = screen.getByText(label)
      expect(badge).toBeInTheDocument()
      expect(badge.className).toContain(tier)
      expect(badge).toHaveAttribute('aria-label', `Risk level: ${label}`)
    })
  })

  it('no longer renders the dead "Always allow" checkbox (U3)', () => {
    mockCtx.permissionRequest = { request_id: 'p1', tool: 'bash', risk: 'low', input: null }
    render(wrap(<Header />, { route: '/chat' }))
    expect(screen.queryByRole('checkbox')).not.toBeInTheDocument()
    expect(screen.queryByText('Always allow')).not.toBeInTheDocument()
  })

  it('focuses Deny so Enter is the safe default and denies the request', async () => {
    mockCtx.permissionRequest = { request_id: 'p9', tool: 'bash', risk: 'high', input: null }
    render(wrap(<Header />, { route: '/chat' }))
    const deny = screen.getByRole('button', { name: 'Deny' })
    expect(deny).toHaveFocus()
    await userEvent.setup().keyboard('{Enter}')
    expect(mockCtx.respondPermission).toHaveBeenCalledWith('p9', false)
  })

  it('clicking Allow Once approves the request', () => {
    mockCtx.permissionRequest = { request_id: 'p9', tool: 'bash', risk: 'high', input: null }
    render(wrap(<Header />, { route: '/chat' }))
    fireEvent.click(screen.getByRole('button', { name: 'Allow Once' }))
    expect(mockCtx.respondPermission).toHaveBeenCalledWith('p9', true)
  })

  it('renders Chat title on /chat route (legacy /goals redirects)', () => {
    render(wrap(<Header />, { route: '/chat' }))
    expect(screen.getByText('Chat')).toBeInTheDocument()
  })

  it('renders Scheduled title on /tasks route', () => {
    render(wrap(<Header />, { route: '/tasks' }))
    expect(screen.getByText('Scheduled')).toBeInTheDocument()
  })

  it('renders Settings title on /settings route', () => {
    render(wrap(<Header />, { route: '/settings/general' }))
    expect(screen.getByText('Settings')).toBeInTheDocument()
  })

  it('renders Extensions title on /extensions route', () => {
    render(wrap(<Header />, { route: '/extensions/skills' }))
    expect(screen.getByText('Extensions')).toBeInTheDocument()
  })

  it('renders notifications and help buttons', () => {
    render(wrap(<Header />, { route: '/chat' }))
    expect(screen.getByLabelText('Notifications')).toBeInTheDocument()
    expect(screen.getByLabelText('Help')).toBeInTheDocument()
  })
})

describe('Header — skill candidate badge', () => {
  beforeEach(() => {
    vi.mocked(api.listSkillCandidates).mockReset()
  })

  it('hides badge when no pending candidates', async () => {
    vi.mocked(api.listSkillCandidates).mockResolvedValue([])
    render(wrap(<Header />, { route: '/chat' }))
    await waitFor(() => { expect(api.listSkillCandidates).toHaveBeenCalled() })
    const bell = screen.getByLabelText('Notifications')
    expect(bell.querySelector('span.bg-error')).toBeNull()
  })

  it('shows count badge when candidates pending', async () => {
    vi.mocked(api.listSkillCandidates).mockResolvedValue([
      { id: 'c1', proposed_name: 'X', proposed_trigger: 'Y', occurrence_count: 1, procedure: [], last_seen_at: '', originating_sessions: [] },
    ])
    render(wrap(<Header />, { route: '/chat' }))
    await waitFor(() => {
      const bell = screen.getByLabelText('Notifications')
      expect(bell.querySelector('.bg-error')?.textContent).toBe('1')
    })
  })

  it('opens SkillApprovalModal on bell click when pending', async () => {
    vi.mocked(api.listSkillCandidates).mockResolvedValue([
      { id: 'c1', proposed_name: 'Wrap commits', proposed_trigger: 'when committing', occurrence_count: 2, procedure: ['s1'], last_seen_at: '', originating_sessions: [] },
    ])
    render(wrap(<Header />, { route: '/chat' }))
    await waitFor(() => { expect(screen.getByLabelText('Notifications').querySelector('.bg-error')).toBeTruthy() })
    fireEvent.click(screen.getByLabelText('Notifications'))
    await waitFor(() => { expect(screen.getByText('Save as skill?')).toBeInTheDocument() })
  })
})
