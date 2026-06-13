import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { AppProvider } from '@/context/AppContext'
import { MemoryRouter } from 'react-router-dom'
import DataSources from '@/components/extensions/DataSources'

const mockCtx = vi.hoisted(() => ({
  mcpServers: [] as any[],
  refreshMcpServers: vi.fn(),
}))

vi.mock('@/context/AppContext', () => ({
  useApp: () => mockCtx,
}))

function wrap(ui: React.ReactElement) {
  return (
    <MemoryRouter>
      {ui}
    </MemoryRouter>
  )
}

describe('DataSources page', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mockCtx.mcpServers = []
    mockCtx.refreshMcpServers.mockResolvedValue(undefined)
  })

  it('renders data sources heading', () => {
    render(wrap(<DataSources />))
    expect(screen.getByText('Data Sources')).toBeInTheDocument()
  })

  it('renders add source button', () => {
    render(wrap(<DataSources />))
    expect(screen.getByText('Add Source')).toBeInTheDocument()
  })

  it('renders empty state when no servers', () => {
    render(wrap(<DataSources />))
    expect(screen.getByText('No MCP servers configured.')).toBeInTheDocument()
  })

  it('shows add form when Add Source is clicked', () => {
    render(wrap(<DataSources />))
    fireEvent.click(screen.getByText('Add Source'))
    expect(screen.getByText('Add MCP Server')).toBeInTheDocument()
    expect(screen.getByPlaceholderText('Name (e.g. my-server)')).toBeInTheDocument()
    expect(screen.getByPlaceholderText('Command (e.g. npx my-mcp-server)')).toBeInTheDocument()
  })

  it('hides add form on Cancel', () => {
    render(wrap(<DataSources />))
    fireEvent.click(screen.getByText('Add Source'))
    expect(screen.getByText('Add MCP Server')).toBeInTheDocument()
    fireEvent.click(screen.getByText('Cancel'))
    expect(screen.queryByText('Add MCP Server')).not.toBeInTheDocument()
  })

  it('calls addMcpServer on form submit', async () => {
    render(wrap(<DataSources />))
    fireEvent.click(screen.getByText('Add Source'))
    fireEvent.change(screen.getByPlaceholderText('Name (e.g. my-server)'), { target: { value: 'test-server' } })
    fireEvent.change(screen.getByPlaceholderText('Command (e.g. npx my-mcp-server)'), { target: { value: 'npx test' } })
    fireEvent.click(screen.getByText('Add Server'))
    const api = await import('@/lib/tauri-api')
    expect(api.addMcpServer).toHaveBeenCalledWith('test-server', 'npx test', [], {})
  })

  it('renders add new source card', () => {
    render(wrap(<DataSources />))
    expect(screen.getByText('Add New Source')).toBeInTheDocument()
  })

  it('renders server cards when servers exist', () => {
    mockCtx.mcpServers = [
      { name: 'test-server', connected: true, tool_count: 5, command: 'npx test' },
    ]
    render(wrap(<DataSources />))
    expect(screen.getByText('test-server')).toBeInTheDocument()
    expect(screen.getByText('Connected')).toBeInTheDocument()
    expect(screen.getByText('5 tools')).toBeInTheDocument()
  })

  it('shows disconnected state for offline servers', () => {
    mockCtx.mcpServers = [
      { name: 'offline-server', connected: false, tool_count: 0, command: 'npx broken' },
    ]
    render(wrap(<DataSources />))
    expect(screen.getByText('offline-server')).toBeInTheDocument()
    expect(screen.getByText('Disconnected')).toBeInTheDocument()
  })

  it('calls restartMcpServer on restart click', async () => {
    mockCtx.mcpServers = [
      { name: 'test-server', connected: true, tool_count: 5, command: 'npx test' },
    ]
    render(wrap(<DataSources />))
    const restartBtn = screen.getByText('sync')
    fireEvent.click(restartBtn)
    const api = await import('@/lib/tauri-api')
    expect(api.restartMcpServer).toHaveBeenCalledWith('test-server')
  })

  it('validates required fields on add', async () => {
    render(wrap(<DataSources />))
    fireEvent.click(screen.getByText('Add Source'))
    fireEvent.click(screen.getByText('Add Server'))
    const api = await import('@/lib/tauri-api')
    expect(api.addMcpServer).not.toHaveBeenCalled()
  })
})
