import '@testing-library/jest-dom/vitest'
import { JSDOM } from 'jsdom'
import { createElement, type ReactElement } from 'react'

// Node >= 25 exposes an experimental global `localStorage` that stays
// `undefined` unless --localstorage-file is passed. That own property on
// globalThis wins over vitest's jsdom global population (which skips keys
// that already exist on global), so every bare `localStorage` in tests
// resolves to undefined (Node <= 22 has no such global, which is why CI
// stays green). Shadow it with a real Storage. `window` is aliased to
// globalThis under vitest's jsdom environment, so the jsdom-window storage
// is unreachable from here — build a throwaway JSDOM window instead.
if (typeof globalThis.localStorage === 'undefined') {
  const storage = new JSDOM('', { url: 'http://localhost/' }).window.localStorage
  delete (globalThis as { localStorage?: unknown }).localStorage
  Object.defineProperty(globalThis, 'localStorage', { configurable: true, value: storage })
}


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

// P1-1 window mode: Layout syncs the native window title via
// getCurrentWindow().setTitle(). The mocked getCurrentWindow hands out the
// same `setTitle` spy on every call, so tests can inspect it through
// `getCurrentWindow().setTitle` after a `mockReset`/`mockClear`.
vi.mock('@tauri-apps/api/window', () => {
  const setTitle = vi.fn().mockResolvedValue(undefined)
  return {
    getCurrentWindow: vi.fn(() => ({
      label: 'session-00000000-0000-0000-0000-000000000000',
      setTitle,
    })),
  }
})

vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: vi.fn().mockResolvedValue(null),
  save: vi.fn().mockResolvedValue(null),
}))

Object.defineProperty(window, 'matchMedia', {
  writable: true,
  value: vi.fn().mockImplementation((query: string) => ({
    matches: /prefers-reduced-motion:\s*reduce/i.test(query),
    media: query,
    onchange: null,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    dispatchEvent: vi.fn(),
  })),
})

// Node >= 24's undici brand-checks the `signal` passed to `new Request()`
// against the Node-global AbortSignal. Vitest's jsdom environment swaps in
// jsdom's realm-local AbortController/AbortSignal, so react-router v7's
// navigation Request construction throws
// "Expected signal to be an instance of AbortSignal" and the navigation
// never happens (Node 22's undici skipped the check, which is why this
// only surfaced once CI moved to Node 24).
//
// The Node-native classes are unreachable from inside the vm context
// (process.getBuiltinModule returns undefined here, and Vite cannot
// externalize node:abort-controller in this setup file), so instead wrap
// the global Request and drop cross-realm signals. Tests never abort
// navigations mid-flight, so an inert abort path is equivalent.
const NativeRequest = globalThis.Request
if (NativeRequest) {
  class RequestWithoutCrossRealmSignal extends NativeRequest {
    constructor(input: RequestInfo | URL, init?: RequestInit) {
      const signal = init?.signal as unknown as { aborted?: boolean } | null | undefined
      if (signal && typeof signal.aborted === 'boolean') {
        const { signal: _crossRealm, ...rest } = init
        super(input, { ...rest, signal: undefined })
        return
      }
      super(input, init)
    }
  }
  globalThis.Request = RequestWithoutCrossRealmSignal as typeof Request
}

class ResizeObserverMock {
  observe = vi.fn()
  unobserve = vi.fn()
  disconnect = vi.fn()
}
global.ResizeObserver = ResizeObserverMock as any

// Mock scrollIntoView for jsdom
Element.prototype.scrollIntoView = vi.fn()

// jsdom doesn't implement scrollTo / scrollHeight / scrollTop uniformly,
// so alias scrollTo and stub the read-only layout properties via
// Object.defineProperty (direct assignment triggers jsdom's strict setter
// guard). Used by StreamingResponse's auto-scroll + jump-to-bottom.
Element.prototype.scrollTo = vi.fn() as unknown as HTMLElement['scrollTo']
Object.defineProperty(HTMLElement.prototype, 'scrollTop', { configurable: true, get() { return 0 }, set() { /* noop */ } })
Object.defineProperty(HTMLElement.prototype, 'scrollLeft', { configurable: true, get() { return 0 }, set() { /* noop */ } })
Object.defineProperty(HTMLElement.prototype, 'scrollHeight', { configurable: true, get() { return 0 } })
Object.defineProperty(HTMLElement.prototype, 'clientHeight', { configurable: true, get() { return 0 } })

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

// Mock tauri-api module. The real module is spread in first so pure
// helpers that don't cross the bridge (e.g. `parseFetchModelsError`)
// behave identically to production; every `invoke`-backing export below
// is overridden with an explicit mock.
vi.mock('@/lib/tauri-api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/lib/tauri-api')>()),
  sendMessage: vi.fn().mockResolvedValue({ message_id: '1', status: 'sent' }),
  // P0-3 preflight — default: every path checks clean; chip-flagging tests
  // override per scenario.
  checkAttachmentPaths: vi.fn().mockResolvedValue([]),
  getConversation: vi.fn().mockResolvedValue([]),
  // A-6 fix default: no active session in the bare render — cold start stays
  // unbound exactly as before; the binding test overrides this per scenario.
  getActiveSessionId: vi.fn().mockResolvedValue(null),
  cancelQuery: vi.fn().mockResolvedValue(undefined),
  // B0 P0-2 — webview file drag-drop. Default: registration resolves with a
  // no-op unlisten and no events ever fire; drag-flow tests override it.
  onWebviewFileDrop: vi.fn().mockResolvedValue(() => {}),
  getConfig: vi.fn().mockResolvedValue({
    provider: 'anthropic',
    model: 'claude-sonnet-4-6',
    api_key: 'sk-test',
    working_dir: '/tmp',
    approval_mode: 'normal',
  }),
  configure: vi.fn().mockResolvedValue(undefined),
  // R3-2: model-profile roster (Settings → Models "Profiles") — one active
  // "default" row by default; flows override per test.
  listProviderProfiles: vi.fn().mockResolvedValue([
    { name: 'default', provider_count: 2, active: true, model: 'claude-sonnet-4-6' },
  ]),
  createProviderProfile: vi.fn(),
  setActiveProviderProfile: vi.fn(),
  // R5: profile rename/delete (Settings → Models "Profiles").
  renameProviderProfile: vi.fn(),
  deleteProviderProfile: vi.fn(),
  // R4-3 (desktop slice): per-provider multi-key management panel.
  listProviderKeys: vi.fn().mockResolvedValue([]),
  addProviderKey: vi.fn(),
  removeProviderKey: vi.fn(),
  activateProviderKey: vi.fn(),
  // P0-③/P1-⑤: plan dock + tool-duration lookup (both opportunistic reads).
  getSessionPlan: vi.fn().mockResolvedValue(null),
  getTraceTimeline: vi.fn().mockResolvedValue({ session_id: 's', turns: [], cumulative: [] }),
  // ADR-0011 B3/B7 — surface identity + bundled CLI install.
  getSurfaceInfo: vi.fn().mockResolvedValue({ surface: 'desktop', version: '0.11.0' }),
  getCliInstallStatus: vi.fn().mockResolvedValue({
    onPath: false,
    onPathVersion: null,
    bundledPath: '/Applications/shannon-desktop.app/Contents/MacOS/shannon',
    handledByInstaller: false,
  }),
  installCliToPath: vi.fn().mockResolvedValue({
    status: { onPath: true, onPathVersion: '0.11.0', bundledPath: null, handledByInstaller: false },
    installedLink: '/usr/local/bin/shannon',
    message: 'linked',
  }),
  // C1① — semi-automatic update check.
  checkAppUpdate: vi.fn().mockResolvedValue({
    currentVersion: '0.11.0',
    latestVersion: 'v0.11.0',
    updateAvailable: false,
    releaseUrl: 'https://github.com/diff-lab-com/shannon-agent/releases',
    error: null,
  }),
  openReleasePage: vi.fn().mockResolvedValue(undefined),
  // Remote targets (SSH hosts / Docker containers). Default: one saved
  // ssh target so the Remotes settings page renders its list.
  remoteListTargets: vi.fn().mockResolvedValue({
    targets: [
      {
        name: 'build-box',
        kind: 'ssh',
        host: 'build-box',
        port: null,
        user: null,
        container: null,
        shell: null,
        sshTarget: null,
        workspaceDir: '/home/ed/proj',
      },
    ],
    // P1-16: the persisted default rides along with the list.
    defaultTarget: 'build-box',
  }),
  remoteDiscoverSshHosts: vi.fn().mockResolvedValue([
    { alias: 'build-box', user: 'ed', hostname: '192.168.1.20', port: 22 },
  ]),
  remoteListDockerContainers: vi.fn().mockResolvedValue([]),
  remoteAddTarget: vi.fn().mockResolvedValue(undefined),
  remoteRemoveTarget: vi.fn().mockResolvedValue(undefined),
  remoteSetDefaultTarget: vi.fn().mockResolvedValue(undefined),
  remoteTestTarget: vi.fn().mockResolvedValue({
    ok: true,
    platform: 'Linux',
    home: '/home/ed',
    bashAvailable: true,
    workspaceExists: true,
    latencyMs: 12,
    error: null,
  }),
  gatewaySetSecret: vi.fn().mockResolvedValue(undefined),
  gatewayGetSecret: vi.fn().mockResolvedValue(null),
  gatewayHasSecret: vi.fn().mockResolvedValue(false),
  gatewayDeleteSecret: vi.fn().mockResolvedValue(undefined),
  gatewayReadConfig: vi.fn().mockResolvedValue({
    engine: { wsUrl: 'ws://127.0.0.1:33420/api/ws', httpBaseUrl: 'http://127.0.0.1:33420' },
    adapters: [],
    // P2-1 — the desktop writes this mobile block by default
    // (commands_mobile_pairing default_mobile_config), so the dispatch card
    // shows a live channel status out of the box.
    mobile: { enabled: true, host: '127.0.0.1', port: 33430 },
  }),
  gatewayWriteConfig: vi.fn().mockResolvedValue({
    engine: { wsUrl: 'ws://127.0.0.1:33420/api/ws', httpBaseUrl: 'http://127.0.0.1:33420' },
    adapters: [],
    mobile: { enabled: true, host: '127.0.0.1', port: 33430 },
  }),
  // E-1 方案 C — default: managed on, not installed (no binary in the test env).
  gatewaySupervisorStart: vi.fn().mockResolvedValue({ managed: true, status: 'notInstalled' }),
  gatewaySupervisorStop: vi.fn().mockResolvedValue({ managed: true, status: 'stopped' }),
  gatewaySupervisorStatus: vi.fn().mockResolvedValue({ managed: true, status: 'stopped' }),
  gatewaySetManaged: vi.fn().mockResolvedValue({ managed: true, status: 'stopped' }),
  // P1.3 — mobile pairing. Default: no devices, a sample token, revoke ok.
  mobileGeneratePairToken: vi.fn().mockResolvedValue({
    token: 'tok-1234',
    expiresAt: Date.now() + 75_000,
    lanEndpoint: 'ws://192.168.1.10:33430',
    qrDataUrl: 'data:image/svg+xml;base64,PHN2Zz4=',
  }),
  mobileListPairedDevices: vi.fn().mockResolvedValue([]),
  mobileRevokeDevice: vi.fn().mockResolvedValue(true),
  mobileTlsStatus: vi.fn().mockResolvedValue({ enabled: false, fingerprint: null }),
  // T9 — gateway IM pairing approval. Default: nothing pending; approve echoes.
  gatewayPairingPending: vi.fn().mockResolvedValue([]),
  gatewayPairingApprove: vi.fn().mockImplementation(async (code: string) => ({
    code,
    platform: 'slack',
    senderId: 'UAPPROVED',
    requestedAt: Date.now(),
    expiresAt: Date.now() + 300_000,
  })),
  testProviderConnection: vi.fn().mockResolvedValue({ kind: 'success' }),
  // P1-7 — settings "send test webhook" one-shot probe.
  testWebhook: vi.fn().mockResolvedValue({ success: true, status: 200, detail: 'HTTP 200' }),
  // 2026-09-29 provider review — in-modal probe + live model listing.
  // Defaults mirror the getConfig default below (a configured provider) so
  // existing gate-dependent tests keep today's behavior; per-test
  // `vi.mocked(...)` overrides cover the unconfigured / failing paths.
  testProviderCredentials: vi.fn().mockResolvedValue({ kind: 'success' }),
  fetchProviderModels: vi.fn().mockResolvedValue([]),
  // Default: configured + keyed, so the ApiKeyBanner / welcome CTA stay
  // hidden in tests that don't care about them (matches the old dead-gate
  // behavior those tests were written against).
  getProviderStatus: vi.fn().mockResolvedValue({
    active_provider_id: 'anthropic-main',
    display_name: 'Anthropic',
    kind: 'anthropic',
    has_api_key: true,
    model: 'claude-sonnet-4-6',
    env_provider: null,
  }),
  listProviders: vi.fn().mockResolvedValue({ active_provider_id: null, providers: [] }),
  saveProvider: vi.fn().mockResolvedValue({ active_provider_id: null, providers: [] }),
  deleteProvider: vi.fn().mockResolvedValue({ active_provider_id: null, providers: [] }),
  setActiveProvider: vi.fn().mockResolvedValue(undefined),
  // ADR-0005 P4.12 — fan-out probe. Default: empty roster.
  testAllProviders: vi.fn().mockResolvedValue([]),
  listModels: vi.fn().mockResolvedValue([
    { id: 'claude-sonnet-4-6', name: 'Claude Sonnet', provider: 'anthropic', context_window: 200000 },
  ]),
  // R2-1 — session model override (composer chip). Default: no override on
  // any session; per-test `vi.mocked(...)` overrides cover the active paths.
  setSessionModel: vi.fn().mockResolvedValue(undefined),
  clearSessionModel: vi.fn().mockResolvedValue(undefined),
  getSessionModel: vi.fn().mockResolvedValue(null),
  // R2-2 — Settings "Refresh model catalog". Default: no-op success.
  refreshModelCatalog: vi.fn().mockResolvedValue({ count: 0, generation: 1 }),
  // ADR-0005 P4.9 — provider allowlist. Default: no override (returns
  // env-var state or null).
  getProviderAllowlist: vi.fn().mockResolvedValue(null),
  getStatus: vi.fn().mockResolvedValue({
    provider: 'anthropic',
    model: 'claude-sonnet-4-6',
    status: 'ready',
  }),
  getTools: vi.fn().mockResolvedValue([]),
  newSession: vi.fn().mockResolvedValue('session-1'),
  listSessions: vi.fn().mockResolvedValue([]),
  searchSessions: vi.fn().mockResolvedValue([]),
  // 卡A archive: the rail's 已归档 lens + archive/restore actions.
  listArchivedSessions: vi.fn().mockResolvedValue([]),
  archiveSession: vi.fn().mockResolvedValue(true),
  unarchiveSession: vi.fn().mockResolvedValue(true),
  // P-E3/P-U3 project registry — default empty so pages degrade to path-tail
  // labels without per-test mocking (the rail tree and the deep-link chips).
  listProjects: vi.fn().mockResolvedValue([]),
  registerProject: vi.fn().mockResolvedValue(null),
  renameProject: vi.fn().mockResolvedValue(null),
  setProjectAppearance: vi.fn().mockResolvedValue(null),
  archiveProject: vi.fn().mockResolvedValue(null),
  unarchiveProject: vi.fn().mockResolvedValue(null),
  loadSession: vi.fn().mockResolvedValue([]),
  switchSession: vi.fn().mockResolvedValue([]),
  setSessionWorkingDir: vi.fn().mockResolvedValue(undefined),
  // P1-1 session multi-window.
  openSessionWindow: vi.fn().mockResolvedValue({ label: 'session-1', sessionId: 'session-1' }),
  listSessionWindows: vi.fn().mockResolvedValue([]),
  closeSessionWindow: vi.fn().mockResolvedValue(undefined),
  revealSessionInMain: vi.fn().mockResolvedValue(undefined),
  createSessionWorktree: vi.fn().mockResolvedValue({ task_id: 's-1', task_name: 'Session', path: '/tmp/wt', branch: 'wt-s-1' }),
  deleteSession: vi.fn().mockResolvedValue(true),
  renameSession: vi.fn().mockResolvedValue(true),
  duplicateSession: vi.fn().mockResolvedValue({ id: 'dup-1', title: 'Copy', created_at: 0 }),
  exportSession: vi.fn().mockResolvedValue(''),
  branchSession: vi.fn().mockResolvedValue({ id: 'branch-1', title: 'Branch', created_at: 0, message_count: 0 }),
  // /rewind + PM-12 feedback
  listCheckpoints: vi.fn().mockResolvedValue([]),
  rewindSession: vi.fn().mockResolvedValue([]),
  listMessageFeedback: vi.fn().mockResolvedValue({}),
  recordMessageFeedback: vi.fn().mockResolvedValue(undefined),
  listFeedbackSessions: vi.fn().mockResolvedValue([]),
  getSessionContextStats: vi.fn().mockResolvedValue({ estimated_tokens: 1200, context_window: 200000 }),
  getSessionUsage: vi.fn().mockResolvedValue({ input_tokens: 100, output_tokens: 50, cache_creation_tokens: 0, cache_read_tokens: 0, cost_usd: 0.01, events: 2 }),
  // P0-4 cost observability
  getSessionBudget: vi.fn().mockResolvedValue(null),
  setSessionBudget: vi.fn().mockResolvedValue(undefined),
  getSessionContextBreakdown: vi.fn().mockResolvedValue({
    totalTokens: 1000,
    contextWindow: 200000,
    categories: [
      { key: 'system', tokens: 200 },
      { key: 'tools', tokens: 300 },
      { key: 'skills', tokens: 0 },
      { key: 'memory', tokens: 0 },
      { key: 'mcp', tokens: 0 },
      { key: 'conversation', tokens: 500 },
    ],
  }),
  getUsageBySession: vi.fn().mockResolvedValue([]),
  getSessionGitDiff: vi.fn().mockResolvedValue({ is_repo: false, files: [], patch: '', truncated: false }),
  compactSession: vi.fn().mockResolvedValue({ performed: true, nothing_to_compact: false, original_tokens: 100, compacted_tokens: 20, reduction_ratio: 0.8, messages_removed: 3, kept_turns: 1, messages: [] }),
  saveTextFile: vi.fn().mockResolvedValue(undefined),
  // P2-2 — persona/profile pack. Defaults are inert (empty counts) so the
  // Settings section renders quietly; per-test `vi.mocked(...)` overrides
  // cover the export / inspect / import flows.
  personaPackExport: vi.fn().mockResolvedValue({
    path: '/tmp/shannon-pack.tar.gz',
    counts: { skills: 0, commands: 0, memories: 0, routines: 0, profiles: 0, persona: 0 },
    stripped: 0,
  }),
  personaPackImport: vi.fn().mockResolvedValue({
    imported: { skills: 0, commands: 0, memories: 0, routines: 0, profiles: 0, persona: 0 },
    skipped: { skills: 0, commands: 0, memories: 0, routines: 0, profiles: 0, persona: 0 },
    failed: [],
  }),
  personaPackInspect: vi.fn().mockResolvedValue({
    version: 1,
    counts: { skills: 0, commands: 0, memories: 0, routines: 0, profiles: 0, persona: 0 },
    createdAtMs: 0,
    generator: 'shannon-test',
  }),
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
  getUsageStats: vi.fn().mockResolvedValue({ days: 30, totals: { label: 'total', input_tokens: 0, output_tokens: 0, cache_creation_tokens: 0, cache_read_tokens: 0, cost_usd: 0, requests: 0 }, by_model: [], by_provider: [], by_day: [] }),
  // P2-1 — usage governance: default null keeps the sidebar % meter and the
  // /usage budget card unmounted in tests that don't care; per-test
  // overrides cover the budgeted / threshold paths.
  getUsageGovernance: vi.fn().mockResolvedValue(null),
  // P2-6 — pre-task cost estimate: default "no history" so the hint renders
  // its first-run copy without per-test mocking.
  estimateTaskCost: vi.fn().mockResolvedValue({ hasHistory: false, runsCounted: 0, minUsd: null, maxUsd: null, avgUsd: null, lastUsd: null }),
  requestPermission: vi.fn().mockResolvedValue(true),
  featuredVendorToEntry: vi.fn().mockResolvedValue({ id: 'test', kind: 'mcp', name: 'Test', description: '', trust: 'community', homepage_url: null, source: null, metadata: {}, tags: [] }),
  sendNotification: vi.fn().mockResolvedValue(undefined),
  getNotificationPrefs: vi.fn().mockResolvedValue({ master_enabled: true, dnd_enabled: false, dnd_start: null, dnd_end: null, on_completed: true, on_failed: true }),
  setNotificationPrefs: vi.fn().mockResolvedValue(undefined),
  getWebhookConfig: vi.fn().mockResolvedValue(null),
  saveWebhookConfig: vi.fn().mockResolvedValue(undefined),
  clearWebhookConfig: vi.fn().mockResolvedValue(undefined),
  // P1-3 — permission profiles, execution mode, sandbox.
  listPermissionProfiles: vi.fn().mockResolvedValue({ builtin: [], custom: [] }),
  activatePermissionProfile: vi.fn().mockResolvedValue({ active: null, approval_mode: null }),
  saveCustomProfile: vi.fn().mockResolvedValue({ name: 'p', description: '', auto_approve: [], confirm: [], deny: [] }),
  deleteCustomProfile: vi.fn().mockResolvedValue([]),
  listHookEvents: vi.fn().mockResolvedValue([]),
  // X6 plugins page — installed management + add-from-three-sources.
  // Defaults keep the installed section quiet; plugin tests override via
  // vi.mocked(...).
  listPlugins: vi.fn().mockResolvedValue([]),
  installPlugin: vi.fn().mockResolvedValue({ name: 'plugin-x', warnings: [] }),
  installPluginFromGit: vi.fn().mockResolvedValue({ name: 'plugin-git', warnings: [] }),
  uninstallPlugin: vi.fn().mockResolvedValue({ warnings: [] }),
  enablePlugin: vi.fn().mockResolvedValue({ warnings: [] }),
  disablePlugin: vi.fn().mockResolvedValue({ warnings: [] }),
  updatePlugin: vi.fn().mockResolvedValue({ warnings: [] }),
  inspectPluginSource: vi.fn().mockResolvedValue({
    name: 'preview-plugin',
    source_format: 'claude-json',
    skills: [],
    agents: [],
    commands: [],
    mcp_servers: [],
  }),
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
  // P2-4 memory provenance + graph — defaults so the Memory page graph tab
  // renders sanely without per-test mocking.
  getMemorySource: vi.fn().mockResolvedValue(null),
  getMemoryGraph: vi.fn().mockResolvedValue({
    project: null, nodes: [], edges: [], entryCount: 0, maxEntries: 200, truncated: false,
  }),
  // Dream pass (梦境提炼) — defaults so the Memory page's distillation
  // section renders its empty state without per-test mocking.
  runDreamPass: vi.fn().mockResolvedValue({
    skipped_reason: null, scanned_sessions: 0, projects: [],
    merge_proposed: 0, remove_proposed: 0, add_proposed: 0,
    candidates_detected: 0, candidates_refined: 0,
    proposal_ids: [], report_path: null, duration_ms: 0,
  }),
  listDreamProposals: vi.fn().mockResolvedValue([]),
  readDreamReport: vi.fn().mockResolvedValue(''),
  applyDreamProposal: vi.fn().mockResolvedValue({ applied: [], skipped: [] }),
  discardDreamProposal: vi.fn().mockResolvedValue(undefined),
  // 卡C — cold-start read-back; null fields keep the 「上次提炼」 line off.
  readDreamState: vi.fn().mockResolvedValue({ last_dream_at: null, last_stats: null }),
  detectSkillsSlash: vi.fn().mockResolvedValue(0),
  // P0-3 inbox — defaults so components consuming useInboxStats (e.g. the
  // sidebar badge) render sanely without per-test mocking.
  listInboxItems: vi.fn().mockResolvedValue([]),
  updateInboxItemStatus: vi.fn().mockResolvedValue(undefined),
  getInboxStats: vi.fn().mockResolvedValue({ pending: 0, today: 0 }),
  rerunInboxItem: vi.fn().mockResolvedValue('run-1'),
  continueInboxItemSession: vi.fn().mockResolvedValue('sess-1'),
  // P0-2 goal runs — default empty so AppProvider's bootstrap and the
  // Tasks-page panel render sanely without per-test mocking.
  listGoalRuns: vi.fn().mockResolvedValue([]),
  getGoalRun: vi.fn().mockResolvedValue(null),
  startGoalRun: vi.fn().mockResolvedValue({ sessionId: 'sess-goal' }),
  // P2-5 off-peak windows — default empty history so the routine drawer's
  // OffpeakWindowEditor renders without a queued status and without
  // per-test mocking.
  listTaskExecutions: vi.fn().mockResolvedValue([]),
  // Tasks page (useScheduledTasks) — default empty so the page and the
  // sidebar automations section render without per-test mocking.
  listScheduledTasks: vi.fn().mockResolvedValue([]),
  // I2 create-schedule path — resolves to a minimal routine so
  // handleCreateSchedule's `created` toast branch works out of the box;
  // tests assert on the CALL args (e.g. the working_dir default).
  createScheduledTask: vi.fn().mockResolvedValue({ id: 'r-mock', name: 'Mock routine', trigger_type: 'interval' }),
  updateScheduledTask: vi.fn().mockResolvedValue(null),
  stopGoalRun: vi.fn().mockResolvedValue(undefined),
  pauseGoalRun: vi.fn().mockResolvedValue(undefined),
  resumeGoalRun: vi.fn().mockResolvedValue(undefined),
  updateGoalObjective: vi.fn().mockResolvedValue(undefined),
  // P1-2 batch runs — default empty so the Tasks-page batch panel stays
  // hidden and other tests are unaffected.
  listBatchRuns: vi.fn().mockResolvedValue([]),
  startBatchRun: vi.fn().mockResolvedValue({ batchId: 'batch-1' }),
  getBatchBranchDiff: vi.fn().mockResolvedValue({ diff: '' }),
  adoptBatchBranch: vi.fn().mockResolvedValue({ merged: true, conflicts: null }),
  discardBatchRun: vi.fn().mockResolvedValue({ removed: 1, skipped: [] }),
  transcribeAudio: vi.fn().mockResolvedValue({ text: 'mock transcript' }),
  // P2-5e — local voice (whisper-rs). Default: returns the same
  // mock transcript as the cloud path so existing tests don't
  // regress; per-test `vi.mocked(...)` overrides cover the
  // STT_* error codes (model-not-found, inference-failed, …).
  transcribeAudioLocal: vi.fn().mockResolvedValue({ text: 'mock transcript' }),
  transcribeAudioLocalBase64: vi.fn().mockResolvedValue({ text: 'mock transcript' }),
  getSttConfig: vi.fn().mockResolvedValue(null),
  saveSttConfig: vi.fn().mockResolvedValue(undefined),
  getVoiceLocalConfig: vi.fn().mockResolvedValue({
    enabled: false,
    model: null,
    language: null,
    auto_download: true,
  }),
  saveVoiceLocalConfig: vi.fn().mockResolvedValue(undefined),
  listWhisperModels: vi.fn().mockResolvedValue([]),
  downloadWhisperModel: vi.fn().mockResolvedValue('/tmp/dummy'),
  deleteWhisperModel: vi.fn().mockResolvedValue(false),
  // P2-5c — attachment uploads. Default: empty payloads, sane cap.
  readAttachment: vi.fn().mockResolvedValue({ mime: 'application/octet-stream', name: '', size: 0 }),
  readAttachments: vi.fn().mockResolvedValue([]),
  MAX_ATTACHMENT_COUNT: 10,
  // LSP quick-fix panel — default to no actions; per-test overrides cover
  // the populated-action / failure paths.
  lspCodeActions: vi.fn().mockResolvedValue({ actions: [] }),
  applyCodeAction: vi.fn().mockResolvedValue(0),
  // 2026-09-25 open pipeline — default: paths don't exist (FileRefChip
  // degrades to inline code), reads/opens inert. Per-test overrides cover
  // the exists → interactive-chip path.
  pathExists: vi.fn().mockResolvedValue(false),
  readTextFile: vi.fn().mockResolvedValue({ path: '', content: '', sizeBytes: 0 }),
  openExternal: vi.fn().mockResolvedValue(undefined),
  openWithDefaultApp: vi.fn().mockResolvedValue(undefined),
  revealInFolder: vi.fn().mockResolvedValue(undefined),
  openArtifactExternally: vi.fn().mockResolvedValue('/tmp/shannon-artifacts/x.html'),
  // Office Wave 1 — host runtime probe (Welcome documents card) + save-as.
  probeHostRuntime: vi.fn().mockResolvedValue({ python3: true, pythonVersion: 'Python 3.12.3', pandoc: false, libreoffice: false }),
  copyFile: vi.fn().mockResolvedValue(undefined),
  probeUrlFrameable: vi.fn().mockResolvedValue({ frameable: true, status: 200, reason: null }),
  // 2026-09-26 round2 §5-1 A — artifact:// interactive HTML registry.
  // Default: one stable registration; per-test overrides cover rejection /
  // fallback paths. The mock is exhaustive — a missing export crashes every
  // test that renders MessageBubble/RightDock.
  registerInteractiveHtml: vi.fn().mockResolvedValue({ id: 'mock-artifact', url: 'artifact://mock-artifact' }),
  unregisterInteractiveArtifact: vi.fn().mockResolvedValue(undefined),
  // P1-5 C-2 — workspace layout persistence. Default: nothing stored, so
  // the Chat page boots on the default focus preset in every test.
  workspaceGetLayout: vi.fn().mockResolvedValue(null),
  workspaceSetLayout: vi.fn().mockResolvedValue(undefined),
  // P1-5 C-2 — preview panel content (LivePreview sync on mount).
  previewStatus: vi.fn().mockResolvedValue({ running: false, url: null, startedAtMs: null }),
  previewDetect: vi.fn().mockResolvedValue({ devServer: null }),
  previewStart: vi.fn().mockResolvedValue({ url: 'http://localhost:5173' }),
  previewStop: vi.fn().mockResolvedValue(undefined),
  previewLogs: vi.fn().mockResolvedValue([]),
  previewCapture: vi.fn().mockResolvedValue({ imageBase64: '', mediaType: 'image/png', width: 1, height: 1 }),
  // P1-5 D — terminal drawer/panel (reconciles on open).
  terminalList: vi.fn().mockResolvedValue([]),
  terminalSpawn: vi.fn().mockResolvedValue({ terminalId: 'term-1' }),
  terminalWrite: vi.fn().mockResolvedValue(undefined),
  terminalResize: vi.fn().mockResolvedValue(undefined),
  terminalKill: vi.fn().mockResolvedValue(undefined),
  // P3-1 — terminal settings card (AdvancedSettings mounts it on every
  // render). set-settings echoes its input like the backend's effective
  // response; history stays an empty replay payload.
  terminalGetSettings: vi.fn().mockResolvedValue({
    shell: null, fontSize: 12, scrollback: 5000, drawerHeight: 320, screenReaderMode: false,
  }),
  terminalSetSettings: vi.fn().mockImplementation((settings: unknown) => Promise.resolve(settings)),
  terminalHistory: vi.fn().mockResolvedValue({ data: '' }),
}))
