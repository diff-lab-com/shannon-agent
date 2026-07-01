import '@testing-library/jest-dom/vitest'
import { createElement, type ReactElement } from 'react'

// Auto-wrap rendered components with I18nProvider so tests don't need to
// manually wrap every `render()` call. This is global; individual tests
// that need a custom locale can still wrap manually.
vi.mock('@testing-library/react', async () => {
  const actual = await vi.importActual<typeof import('@testing-library/react')>('@testing-library/react')
  const { I18nProvider } = await import('@/i18n')
  const wrap = (ui: ReactElement) => createElement(I18nProvider, null, ui)
  return {
    ...actual,
    render: (ui: ReactElement, options?: Parameters<typeof actual.render>[1]) => {
      const result = actual.render(wrap(ui), options)
      // Also wrap `rerender` — it bypasses render() and would otherwise drop
      // the provider (e.g. open→closed Modal transitions inside tests).
      const originalRerender = result.rerender
      result.rerender = (
        rerenderUi: ReactElement,
        rerenderOptions?: Parameters<typeof originalRerender>[1],
      ) => originalRerender(wrap(rerenderUi), rerenderOptions)
      return result
    },
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

// jsdom has no PointerEvent constructor; base-ui's Switch onClick constructs
// `new ownerWindow(input).PointerEvent(...)` (to tell pointer vs keyboard
// activation). Stub it as a MouseEvent subclass so switch toggles work.
class PointerEventMock extends MouseEvent {}
;(globalThis as any).PointerEvent = PointerEventMock
;(window as any).PointerEvent = PointerEventMock

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
  gatewaySetSecret: vi.fn().mockResolvedValue(undefined),
  gatewayGetSecret: vi.fn().mockResolvedValue(null),
  gatewayHasSecret: vi.fn().mockResolvedValue(false),
  gatewayDeleteSecret: vi.fn().mockResolvedValue(undefined),
  gatewayReadConfig: vi.fn().mockResolvedValue({
    engine: { wsUrl: 'ws://127.0.0.1:33420/api/ws', httpBaseUrl: 'http://127.0.0.1:33420' },
    adapters: [],
  }),
  gatewayWriteConfig: vi.fn().mockResolvedValue({
    engine: { wsUrl: 'ws://127.0.0.1:33420/api/ws', httpBaseUrl: 'http://127.0.0.1:33420' },
    adapters: [],
  }),
  // E-1 方案 C — default: managed on, not installed (no binary in the test env).
  gatewaySupervisorStart: vi.fn().mockResolvedValue({ managed: true, status: 'notInstalled' }),
  gatewaySupervisorStop: vi.fn().mockResolvedValue({ managed: true, status: 'stopped' }),
  gatewaySupervisorStatus: vi.fn().mockResolvedValue({ managed: true, status: 'stopped' }),
  gatewaySetManaged: vi.fn().mockResolvedValue({ managed: true, status: 'stopped' }),
  switchProvider: vi.fn().mockResolvedValue(undefined),
  testProviderConnection: vi.fn().mockResolvedValue({ kind: 'success' }),
  listProviders: vi.fn().mockResolvedValue({ active_provider_id: null, providers: [] }),
  saveProvider: vi.fn().mockResolvedValue({ active_provider_id: null, providers: [] }),
  deleteProvider: vi.fn().mockResolvedValue({ active_provider_id: null, providers: [] }),
  setActiveProvider: vi.fn().mockResolvedValue(undefined),
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
  createSessionWorktree: vi.fn().mockResolvedValue({ task_id: 's-1', task_name: 'Session', path: '/tmp/wt', branch: 'wt-s-1' }),
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
  getUsageStats: vi.fn().mockResolvedValue({ days: 30, totals: { label: 'total', input_tokens: 0, output_tokens: 0, cache_creation_tokens: 0, cache_read_tokens: 0, cost_usd: 0, requests: 0 }, by_model: [], by_provider: [], by_day: [] }),
  requestPermission: vi.fn().mockResolvedValue(true),
  featuredVendorToEntry: vi.fn().mockResolvedValue({ id: 'test', kind: 'mcp', name: 'Test', description: '', trust: 'community', homepage_url: null, source: null, metadata: {}, tags: [] }),
  sendNotification: vi.fn().mockResolvedValue(undefined),
  getNotificationPrefs: vi.fn().mockResolvedValue({ master_enabled: true, dnd_enabled: false, dnd_start: null, dnd_end: null, on_completed: true, on_failed: true }),
  setNotificationPrefs: vi.fn().mockResolvedValue(undefined),
  listPluginMarketplace: vi.fn().mockResolvedValue([]),
  listCatalogUpstreams: vi.fn().mockResolvedValue([]),
  installSkillFromRepo: vi.fn().mockResolvedValue({ id: 'skill-1', name: 'Test Skill', install_path: '/path/to/skill' }),
  installAgentFromRepo: vi.fn().mockResolvedValue({ id: 'agent-1', name: 'Test Agent', install_path: '/path/to/agent' }),
  listSkillCandidates: vi.fn().mockResolvedValue([]),
  approveSkillCandidate: vi.fn().mockResolvedValue({ id: 'skill-x', name: '', description: '', trigger: '', procedure: [], created_at: '', originating_sessions: [] }),
  rejectSkillCandidate: vi.fn().mockResolvedValue(undefined),
  listAgentAuthoredSkills: vi.fn().mockResolvedValue([]),
  listDataSourceCatalog: vi.fn().mockResolvedValue([]),
  listInstalledDataSources: vi.fn().mockResolvedValue([]),
  queryDataSource: vi.fn().mockResolvedValue({ items: [], total: 0, has_more: false }),
  listRoutineTemplates: vi.fn().mockResolvedValue([]),
  instantiateRoutineTemplate: vi.fn().mockResolvedValue({ id: 'test', name: 'Test' }),
  listMemoryProjects: vi.fn().mockResolvedValue([]),
  listMemories: vi.fn().mockResolvedValue([]),
  createMemory: vi.fn().mockResolvedValue({
    id: 'mem-1', project: '.', category: 'context', content: '',
    tags: [], confidence: 1.0, created_at: '', accessed_at: '', access_count: 0,
  }),
  updateMemory: vi.fn().mockResolvedValue({
    id: 'mem-1', project: '.', category: 'context', content: '',
    tags: [], confidence: 1.0, created_at: '', accessed_at: '', access_count: 0,
  }),
  deleteMemory: vi.fn().mockResolvedValue(true),
  searchMemories: vi.fn().mockResolvedValue([]),
  getMemoryStats: vi.fn().mockResolvedValue({
    total: 0, by_category: {}, by_project: {}, most_recent_at: null,
  }),
  markTriageRead: vi.fn().mockResolvedValue(undefined),
  archiveTriageItem: vi.fn().mockResolvedValue(undefined),
  transcribeAudio: vi.fn().mockResolvedValue({ text: 'mock transcript' }),
  getSttConfig: vi.fn().mockResolvedValue(null),
  saveSttConfig: vi.fn().mockResolvedValue(undefined),
}))
