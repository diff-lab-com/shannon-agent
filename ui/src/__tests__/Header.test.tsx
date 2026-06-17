import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { I18nProvider } from '@/i18n'
import { Header } from '@/components/Header'

const mockCtx = vi.hoisted(() => ({
  status: { model: 'claude-sonnet-4-6', provider: 'anthropic', querying: false } as any,
  models: [
    { id: 'claude-sonnet-4-6', name: 'Claude Sonnet', provider: 'anthropic', context_window: 200000 },
    { id: 'gpt-4o', name: 'GPT-4o', provider: 'openai', context_window: 128000 },
  ],
  permissionRequest: null as any,
  respondPermission: vi.fn(),
}))

vi.mock('@/context/AppContext', () => ({
  useApp: () => mockCtx,
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
  })

  it('renders page title based on route', () => {
    render(wrap(<Header />, { route: '/chat' }))
    expect(screen.getByText('Chat')).toBeInTheDocument()
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

  it('switches model when option is clicked', async () => {
    const api = await import('@/lib/tauri-api')
    render(wrap(<Header />, { route: '/chat' }))
    fireEvent.click(screen.getByText('claude-sonnet-4-6'))
    await waitFor(() => {
      expect(screen.getByText('GPT-4o')).toBeInTheDocument()
    })
    fireEvent.click(screen.getByText('GPT-4o'))
    expect(api.switchProvider).toHaveBeenCalled()
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
