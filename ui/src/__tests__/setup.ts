import '@testing-library/jest-dom/vitest'
import { createElement, type ReactElement } from 'react'

// Auto-wrap rendered components with I18nProvider so tests don't need to
// manually wrap every `render()` call. This is global; individual tests
// that need a custom locale can still wrap manually.
vi.mock('@testing-library/react', async () => {
  const actual = await vi.importActual<typeof import('@testing-library/react')>('@testing-library/react')
  const { I18nProvider } = await import('@/i18n')
  return {
    ...actual,
    render: (ui: ReactElement, options?: Parameters<typeof actual.render>[1]) =>
      actual.render(createElement(I18nProvider, null, ui), options),
  }
})

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
  convertFileSrc: (path: string) => `asset://localhost/${path.replace(/^\//, '')}`,
}))

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
  emit: vi.fn(),
}))

vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: vi.fn().mockResolvedValue(null),
  save: vi.fn().mockResolvedValue(null),
}))

Object.defineProperty(window, 'matchMedia', {
  writable: true,
  value: vi.fn().mockImplementation((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    dispatchEvent: vi.fn(),
  })),
})

class ResizeObserverMock {
  observe = vi.fn()
  unobserve = vi.fn()
  disconnect = vi.fn()
}
global.ResizeObserver = ResizeObserverMock as any

// Mock scrollIntoView for jsdom
Element.prototype.scrollIntoView = vi.fn()

// Mock getAnimations for base-ui ScrollArea
Element.prototype.getAnimations = vi.fn().mockReturnValue([])

class IntersectionObserverMock {
  readonly root = null
  readonly rootMargin = ''
  readonly thresholds = []
  observe = vi.fn()
  unobserve = vi.fn()
  disconnect = vi.fn()
  takeRecords = vi.fn().mockReturnValue([])
}
global.IntersectionObserver = IntersectionObserverMock as any

// Mock tauri-api module
vi.mock('@/lib/tauri-api', () => ({
  sendMessage: vi.fn().mockResolvedValue({ message_id: '1', status: 'sent' }),
  getConversation: vi.fn().mockResolvedValue([]),
  cancelQuery: vi.fn().mockResolvedValue(undefined),
  getConfig: vi.fn().mockResolvedValue({
    provider: 'anthropic',
    model: 'claude-sonnet-4-6',
    api_key: 'sk-test',
    working_dir: '/tmp',
    approval_mode: 'normal',
  }),
  configure: vi.fn().mockResolvedValue(undefined),
  switchProvider: vi.fn().mockResolvedValue(undefined),
  listModels: vi.fn().mockResolvedValue([
    { id: 'claude-sonnet-4-6', name: 'Claude Sonnet', provider: 'anthropic', context_window: 200000 },
  ]),
  getStatus: vi.fn().mockResolvedValue({
    provider: 'anthropic',
    model: 'claude-sonnet-4-6',
    status: 'ready',
  }),
  getTools: vi.fn().mockResolvedValue([]),
  newSession: vi.fn().mockResolvedValue('session-1'),
  listSessions: vi.fn().mockResolvedValue([]),
  searchSessions: vi.fn().mockResolvedValue([]),
  loadSession: vi.fn().mockResolvedValue([]),
  switchSession: vi.fn().mockResolvedValue([]),
  setSessionWorkingDir: vi.fn().mockResolvedValue(undefined),
  deleteSession: vi.fn().mockResolvedValue(true),
  renameSession: vi.fn().mockResolvedValue(true),
  duplicateSession: vi.fn().mockResolvedValue({ id: 'dup-1', title: 'Copy', created_at: 0 }),
  exportSession: vi.fn().mockResolvedValue(''),
  branchSession: vi.fn().mockResolvedValue({ id: 'branch-1', title: 'Branch', created_at: 0, message_count: 0 }),
  saveTextFile: vi.fn().mockResolvedValue(undefined),
  respondPermission: vi.fn().mockResolvedValue(undefined),
  getFileDiff: vi.fn().mockResolvedValue({ path: '', hunks: [] }),
  applyDiff: vi.fn().mockResolvedValue(undefined),
  getFileTree: vi.fn().mockResolvedValue({ name: 'root', path: '/', is_dir: true, children: [] }),
  getWorkingDirInfo: vi.fn().mockResolvedValue({ path: '/tmp', name: 'tmp' }),
  listMcpServers: vi.fn().mockResolvedValue([]),
  addMcpServer: vi.fn().mockResolvedValue({ name: 'test', command: 'test', enabled: true, connected: false, tool_count: 0, tools: [], last_connected: null }),
  removeMcpServer: vi.fn().mockResolvedValue(true),
  restartMcpServer: vi.fn().mockResolvedValue({ name: 'test', command: 'test', enabled: true, connected: true, tool_count: 0, tools: [], last_connected: null }),
  getMcpServerConfig: vi.fn().mockResolvedValue({ name: 'test', command: 'test', args: [], env: {} }),
  listSkills: vi.fn().mockResolvedValue([]),
  getSkillDetail: vi.fn().mockResolvedValue({ name: 'test', description: '', source: '', trigger: '' }),
  startBackgroundTask: vi.fn().mockResolvedValue('task-1'),
  getBackgroundTasks: vi.fn().mockResolvedValue([]),
  cancelBackgroundTask: vi.fn().mockResolvedValue(true),
  listAgents: vi.fn().mockResolvedValue([]),
  listTasks: vi.fn().mockResolvedValue([]),
  getBillingPlan: vi.fn().mockResolvedValue({ name: 'Free', price: 0, token_limit: 100000, features: ['Basic models', '5 sessions'] }),
  getCostHistory: vi.fn().mockResolvedValue([]),
  getBillingHistory: vi.fn().mockResolvedValue([]),
  requestPermission: vi.fn().mockResolvedValue(true),
  featuredVendorToEntry: vi.fn().mockResolvedValue({ id: 'test', kind: 'mcp', name: 'Test', description: '', trust: 'community', homepage_url: null, source: null, metadata: {}, tags: [] }),
  sendNotification: vi.fn().mockResolvedValue(undefined),
  getInboundConfig: vi.fn().mockResolvedValue({ slack: null, telegram: null }),
  saveInboundConfig: vi.fn().mockResolvedValue(undefined),
  clearInboundConfig: vi.fn().mockResolvedValue(undefined),
  getInboundListenerStatus: vi.fn().mockResolvedValue({ slack_running: false, telegram_running: false }),
  stopInboundListener: vi.fn().mockResolvedValue(undefined),
  listPluginMarketplace: vi.fn().mockResolvedValue([]),
  listCatalogUpstreams: vi.fn().mockResolvedValue([]),
  installSkillFromRepo: vi.fn().mockResolvedValue({ id: 'skill-1', name: 'Test Skill', install_path: '/path/to/skill' }),
  installAgentFromRepo: vi.fn().mockResolvedValue({ id: 'agent-1', name: 'Test Agent', install_path: '/path/to/agent' }),
  listDataSourceCatalog: vi.fn().mockResolvedValue([]),
  listInstalledDataSources: vi.fn().mockResolvedValue([]),
  queryDataSource: vi.fn().mockResolvedValue({ items: [], total: 0, has_more: false }),
}))
