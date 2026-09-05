// Mock command handlers — map every Tauri command used by tauri-api.ts to a mock response.
// Handlers are async to mimic network latency. All return clones so consumers can't mutate the data.
import { MOCK_TASKS, MOCK_AGENTS, MOCK_AGENT_DEFINITIONS, MOCK_SESSIONS, MOCK_MESSAGES,
  MOCK_SKILLS, MOCK_MCP_SERVERS, MOCK_PLUGINS, MOCK_BACKGROUND_TASKS,
  MOCK_TURN_TIMELINE } from './data/core'
import { MOCK_SCHEDULED_ROUTINES, MOCK_TRIGGERED_ROUTINES, MOCK_HOOK_EVENTS, MOCK_PROFILES } from './data/automation'
import { MOCK_INBOX_ITEMS, MOCK_OPC_METRICS, MOCK_PERF_TRACES, MOCK_DIAGNOSTICS,
  MOCK_CODE_ACTIONS, MOCK_GOALS } from './data/analytics'
import { MOCK_CONFIG, MOCK_MODELS, MOCK_STATUS, MOCK_TOOLS, MOCK_PROVIDERS } from './data/config'
import type { InboxItem, ProviderInput, SessionInfo } from '@/types'
import { MOCK_MEMORIES, MOCK_MEMORY_PROJECTS, MOCK_MEMORY_STATS, MOCK_FEATURED_VENDORS } from './data/memory'
import {
  MOCK_SKILL_CATALOG,
  MOCK_AGENT_CATALOG,
  MOCK_INSTALLED_SKILLS,
  MOCK_INSTALLED_AGENTS,
  MOCK_INSTALLED_ADDONS,
} from './data/catalog'

const clone = <T,>(v: T): T => JSON.parse(JSON.stringify(v))
const delay = (ms = 80) => new Promise<void>(r => setTimeout(r, ms + Math.random() * 40))

// Demo-build session mutations (see list_sessions above).
const deletedSessions = new Set<string>()
const renamedSessions = new Map<string, SessionInfo>()

// P1-3: mutable desktop config so execution-mode / sandbox switches in the
// demo feel live (get_config hands out a fresh clone of this).
const demoConfig = clone(MOCK_CONFIG)

// Mutable state for "live" feeling during demo
const state = {
  tasks: clone(MOCK_TASKS),
  scheduled: clone(MOCK_SCHEDULED_ROUTINES),
  background: clone(MOCK_BACKGROUND_TASKS),
  providers: clone(MOCK_PROVIDERS),
  inbox: clone(MOCK_INBOX_ITEMS) as InboxItem[],
}

// ids for inbox items created at runtime (rerun simulation).
let nextInboxId = Math.max(...MOCK_INBOX_ITEMS.map(i => i.id)) + 1

// P0-4: demo session budget — null = no cap; set via the budget control.
let demoBudgetUsd: number | null = null

// P1-5 C-1: demo live-preview lifecycle (single instance, like the backend).
const demoPreview = {
  running: false,
  url: null as string | null,
  startedAtMs: null as number | null,
}
const PREVIEW_URL = 'http://localhost:5173'
// 1x1 transparent PNG so demo capture payloads stay a real image.
const PREVIEW_PNG = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg=='

// P1-2: demo best-of-N batch runs. One running + one finished so the Tasks
// page batch cards and the compare dialog both have something to show.
const demoBatchBranch = (i: number, status: string, files: number, spent: number, err: string | null = null) => ({
  index: i,
  branchName: `batch-demo000${i}-${i}`,
  worktreePath: `/tmp/demo-repo/.shannon/scheduled-worktrees/batch-demo000${i}-${i}`,
  status,
  error: err,
  summary: status === 'running' ? null : { filesChanged: files, additions: files * 7, deletions: files * 2 },
  spentUsd: spent,
})
const batchRuns: Record<string, unknown>[] = [
  {
    batchId: '0196batch-0000-7000-8000-000000000001',
    title: 'Speed up the search box',
    prompt: 'Reduce search-as-you-type latency; consider caching and debounce',
    count: 3,
    status: 'running',
    createdAtMs: Date.now() - 4 * 60_000,
    branches: [
      demoBatchBranch(0, 'completed', 4, 0.31),
      demoBatchBranch(1, 'running', 0, 0.12),
      demoBatchBranch(2, 'failed', 0, 0.05, 'provider overloaded (429)'),
    ],
    adoptedIndex: null,
  },
  {
    batchId: '0196batch-0000-7000-8000-000000000002',
    title: 'Add CSV export to reports',
    prompt: 'Add an export button that downloads the filtered report as CSV',
    count: 2,
    status: 'completed',
    createdAtMs: Date.now() - 40 * 60_000,
    branches: [demoBatchBranch(0, 'completed', 6, 0.44), demoBatchBranch(1, 'completed', 3, 0.27)],
    adoptedIndex: null,
  },
]
const demoPatch = (branch: number) =>
  [
    'diff --git a/src/search.ts b/src/search.ts',
    'index 83db48f..bf269f4 100644',
    '--- a/src/search.ts',
    '+++ b/src/search.ts',
    '@@ -12,7 +12,10 @@ export function createSearchBox() {',
    '   const cache = new Map<string, Result>()',
    '+  // branch #' + branch + ': debounce keystrokes before hitting the index',
    '+  let timer: number | undefined',
    '   input.addEventListener("input", () => {',
    '-    runSearch(input.value)',
    '+    clearTimeout(timer)',
    '+    timer = setTimeout(() => runSearch(input.value), 120)',
    '   })',
  ].join('\n')

// P0-2: demo goal runs. One live (so the Tasks page shows a run card) and
// one finished; start_goal_run appends new running rows with live feel.
const goalRuns = [
  {
    sessionId: '0196aaaa-0000-7000-8000-000000000001',
    title: 'Harden the upload pipeline',
    objective: 'Add retry + tests to the upload pipeline so flaky network errors cannot lose files',
    status: 'running',
    iterations: 3,
    maxTurns: 12,
    spentUsd: 0.42,
    budgetUsd: 5,
    stallStrikes: 0,
    lastError: null,
    startedAtMs: Date.now() - 26 * 60_000,
    updatedAtMs: Date.now() - 2 * 60_000,
  },
  {
    sessionId: '0196aaaa-0000-7000-8000-000000000002',
    title: 'Changelog digest',
    objective: 'Summarize the last two weeks of commits into a release-notes draft',
    status: 'completed',
    iterations: 4,
    maxTurns: null,
    spentUsd: 0.18,
    budgetUsd: null,
    stallStrikes: 0,
    lastError: null,
    startedAtMs: Date.now() - 27 * 60 * 60_000,
    updatedAtMs: Date.now() - 26.5 * 60 * 60_000,
  },
] as Array<Record<string, unknown> & { sessionId: string; status: string }>

// Snapshot the managed-providers roster as a cloned ProvidersFile.
function providersFile() {
  return clone(state.providers)
}

function findTask(id: string) {
  return state.tasks.find(t => t.id === id)
}

// Mutable notification prefs so DND/quiet-hours toggling feels live in demo mode.
let notificationPrefs = {
  master_enabled: true,
  dnd_enabled: false,
  dnd_start: null as string | null,
  dnd_end: null as string | null,
  on_completed: true,
  on_failed: true,
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type MockHandler = (args: any) => unknown | Promise<unknown>
export const handlers: Record<string, MockHandler> = {
  // --- Chat ---
  async send_message() {
    await delay(120)
    return { query_id: `q-${Date.now()}` }
  },
  async get_conversation() {
    await delay()
    return clone(MOCK_MESSAGES)
  },
  async cancel_query() { await delay(30) },

  // --- Config ---
  async get_config() { await delay(); return clone(demoConfig) },
  async configure(args: { key: string; value: string }) {
    await delay()
    // P1-3: keep the persisted keys the new settings surfaces touch in sync.
    if (args?.key === 'sandbox.mode') {
      const mode = String(args.value || 'off') as 'off' | 'local' | 'landlock'
      demoConfig.sandbox = { mode }
    } else if (args?.key === 'approval_mode') {
      demoConfig.approval_mode = args.value
    }
  },

  // --- Managed providers (Models P2) ---
  async test_provider_connection() {
    // Demo can't reach a real backend, so every probe reports success —
    // enough to exercise the success toast and the Test button state.
    await delay(400)
    return { kind: 'success' }
  },
  async list_providers() { await delay(); return providersFile() },
  async save_provider(args: { input: ProviderInput }) {
    await delay(120)
    const input = args.input
    const existing = input.id
      ? state.providers.providers.find(p => p.id === input.id)
      : undefined
    if (existing) {
      // Edit: keep the stored key when the frontend re-submits the mask.
      const keepKey = !input.api_key || input.api_key === '***'
      Object.assign(existing, {
        display_name: input.display_name,
        kind: input.kind,
        has_api_key: keepKey ? existing.has_api_key : !!input.api_key,
        base_url: input.base_url || null,
      })
    } else {
      state.providers.providers.push({
        id: `prov-${Date.now()}`,
        display_name: input.display_name,
        kind: input.kind,
        has_api_key: !!input.api_key,
        base_url: input.base_url || null,
      })
    }
    return providersFile()
  },
  async delete_provider(args: { id: string }) {
    await delay(100)
    state.providers.providers = state.providers.providers.filter(p => p.id !== args.id)
    if (state.providers.active_provider_id === args.id) {
      state.providers.active_provider_id = null
    }
    return providersFile()
  },
  async set_active_provider(args: { id: string }) {
    await delay(150)
    state.providers.active_provider_id = args.id
  },

  // --- Models & Status ---
  async list_models() { await delay(); return clone(MOCK_MODELS) },
  async get_status() { await delay(40); return clone(MOCK_STATUS) },
  async list_tools() { await delay(); return clone(MOCK_TOOLS) },

  // --- Sessions ---
  // Deleted ids / renamed titles are tracked so the demo build and e2e flows
  // observe their own mutations (list/search reflect them on refresh).
  async new_session() { await delay(60); return `sess-${Date.now()}` },
  async list_sessions() {
    await delay()
    return clone(MOCK_SESSIONS)
      .filter(s => !deletedSessions.has(s.id))
      .map(s => renamedSessions.get(s.id) ?? s)
  },
  async search_sessions(args: { query: string }) {
    await delay()
    const q = (args.query ?? '').toLowerCase()
    return clone(MOCK_SESSIONS)
      .filter(s => !deletedSessions.has(s.id))
      .map(s => renamedSessions.get(s.id) ?? s)
      .filter(s => s.title.toLowerCase().includes(q))
  },
  async load_session() { await delay(); return clone(MOCK_MESSAGES) },
  async switch_session() { await delay(); return clone(MOCK_MESSAGES) },
  async delete_session(args: { id: string }) { await delay(60); deletedSessions.add(args.id); return true },
  async rename_session(args: { id: string; title: string }) {
    await delay(60)
    const base = renamedSessions.get(args.id) ?? MOCK_SESSIONS.find(s => s.id === args.id)
    if (base) renamedSessions.set(args.id, { ...base, title: args.title })
    return true
  },
  async duplicate_session(args: { id: string }) {
    await delay(60)
    const src = MOCK_SESSIONS.find(s => s.id === args.id)
    return src ? { ...clone(src), id: `sess-${Date.now()}`, title: `${src.title} copy` } : null
  },
  async export_session() { await delay(120); return '# Exported session\n\n(mock content)' },
  // ── Remote targets (SSH hosts / Docker containers) ──
  async remote_list_targets() {
    await delay()
    return [
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
      {
        name: 'ci-runner',
        kind: 'docker',
        host: null,
        port: null,
        user: null,
        container: 'shannon-ci',
        shell: 'bash',
        sshTarget: 'build-box',
        workspaceDir: '/workspace',
      },
    ]
  },
  async remote_discover_ssh_hosts() {
    await delay()
    return [
      { alias: 'build-box', user: 'ed', hostname: '192.168.1.20', port: 22 },
      { alias: 'gpu-1', user: 'deploy', hostname: null, port: null },
    ]
  },
  async remote_list_docker_containers() {
    await delay(140)
    return [
      { id: 'a1b2c3', names: 'shannon-ci', image: 'ubuntu:22.04', status: 'Up 3 hours' },
      { id: 'd4e5f6', names: 'dev-sandbox', image: 'node:20', status: 'Up 20 minutes' },
    ]
  },
  async remote_add_target(_args: { target: unknown }) {
    await delay(120)
    return null
  },
  async remote_remove_target(_args: { name: string }) {
    await delay(80)
    return null
  },
  async remote_set_default_target(_args: { name: string | null }) {
    await delay(60)
    return null
  },
  async remote_test_target(_args: { name: string }) {
    await delay(300)
    return {
      ok: true,
      platform: 'Linux',
      home: '/home/ed',
      bashAvailable: true,
      workspaceExists: true,
      latencyMs: 24,
      error: null,
    }
  },
  // /rewind: demo has no checkpoints (record_turn runs in the desktop Rust
  // process), so the rewind affordance stays hidden and these are safety nets.
  async list_checkpoints() { await delay(30); return [] },
  async list_message_feedback() { await delay(30); return {} },
  async record_message_feedback() { await delay(30) },
  async list_feedback_sessions() { await delay(30); return [] },
  async rewind_session() { await delay(80); return clone(MOCK_MESSAGES) },
  // Slash-command backends: /context and /cost return readable demo numbers,
  // /diff reports a non-repo so the demo composer shows the calm notice.
  async get_session_context_stats() {
    await delay(30)
    return { estimated_tokens: 4820, context_window: 200000 }
  },
  async get_session_usage() {
    await delay(30)
    return { input_tokens: 12400, output_tokens: 3150, cache_creation_tokens: 0, cache_read_tokens: 9800, cost_usd: 0.0731, events: 6 }
  },
  // P0-4 cost observability: demo budget (mutable so the banner flow is
  // explorable), a fixed six-category breakdown and two attributed
  // sessions for the Usage page's per-session view.
  async get_session_budget() { await delay(30); return demoBudgetUsd },
  async set_session_budget(args: { budgetUsd: number | null }) {
    await delay(30)
    demoBudgetUsd = args.budgetUsd
  },
  async get_session_context_breakdown() {
    await delay(30)
    return {
      totalTokens: 9480,
      contextWindow: 200000,
      categories: [
        { key: 'system', tokens: 1820 },
        { key: 'tools', tokens: 2640 },
        { key: 'skills', tokens: 610 },
        { key: 'memory', tokens: 340 },
        { key: 'mcp', tokens: 0 },
        { key: 'conversation', tokens: 4070 },
      ],
    }
  },
  async get_usage_by_session(args: { days: number }) {
    await delay()
    const cutoff = Date.now() - Math.min(args.days ?? 30, 365) * 86400_000
    const rows = [
      { sessionId: MOCK_SESSIONS[0]?.id ?? 'demo-session', title: MOCK_SESSIONS[0]?.title ?? null, inputTokens: 48210, outputTokens: 12640, cacheCreationTokens: 18300, cacheReadTokens: 156400, costUsd: 0.842, requests: 31, lastUsedAtMs: Date.now() - 3600_000 },
      { sessionId: MOCK_SESSIONS[1]?.id ?? 'demo-session-2', title: MOCK_SESSIONS[1]?.title ?? null, inputTokens: 15400, outputTokens: 8210, cacheCreationTokens: 4200, cacheReadTokens: 38700, costUsd: 0.214, requests: 12, lastUsedAtMs: Date.now() - 26 * 3600_000 },
      { sessionId: '8f2c1a9e-4b7d-4c3a-9f01-2d5e8b7a6c01', title: null, inputTokens: 6100, outputTokens: 2400, cacheCreationTokens: 0, cacheReadTokens: 0, costUsd: 0.038, requests: 4, lastUsedAtMs: Date.now() - 20 * 86400_000 },
    ]
    return rows.filter(r => r.lastUsedAtMs >= cutoff)
  },
  async get_session_git_diff() {
    await delay(30)
    return { is_repo: false, files: [], patch: '', truncated: false }
  },
  async compact_session() {
    await delay(400)
    return {
      performed: true, nothing_to_compact: false,
      original_tokens: 4820, compacted_tokens: 960, reduction_ratio: 0.8,
      messages_removed: 11, kept_turns: 2,
      messages: clone(MOCK_MESSAGES).slice(0, 2),
    }
  },
  // §4.14 — Turn Timeline: demo data regardless of id (sessions come from
  // MOCK_SESSIONS, which do not have real L0 logs to project).
  async trace_timeline() { await delay(); return clone(MOCK_TURN_TIMELINE) },

  // --- Permissions ---
  async respond_permission() { await delay(20) },

  // --- Files ---
  async get_file_diff(args: { path: string }) {
    await delay()
    return {
      old_content: `// old content of ${args.path}\nfn main() { println!("hi"); }`,
      new_content: `// new content of ${args.path}\nfn main() { println!("hello world"); }`,
      file_name: args.path.split('/').pop() ?? args.path,
      language: 'rust',
    }
  },
  async apply_diff() { await delay(100) },
  async get_file_tree() {
    await delay()
    return {
      name: 'workspace',
      path: '/Users/demo/workspace/my-startup',
      type: 'directory',
      children: [
        { name: 'src', path: 'src', type: 'directory', children: [
          { name: 'main.rs', path: 'src/main.rs', type: 'file', size: 4200 },
          { name: 'lib.rs', path: 'src/lib.rs', type: 'file', size: 1800 },
        ]},
        { name: 'README.md', path: 'README.md', type: 'file', size: 2400 },
      ],
    }
  },
  async get_working_dir_info() {
    await delay()
    return {
      root: '/Users/demo/workspace/my-startup',
      branch: 'feature/billing-v2',
      modified_files: ['src/billing/invoice.rs', 'src/webhooks/stripe.rs', 'README.md'],
      status: 'dirty',
    }
  },

  // --- MCP ---
  async list_mcp_servers() { await delay(); return clone(MOCK_MCP_SERVERS) },
  async add_mcp_server(args: { name: string }) {
    await delay(200)
    return { name: args.name, command: '', enabled: true, connected: false, tool_count: 0, tools: [], last_connected: null }
  },
  async remove_mcp_server() { await delay() ; return true },
  async restart_mcp_server(args: { name: string }) {
    await delay(400)
    const srv = MOCK_MCP_SERVERS.find(s => s.name === args.name)
    return srv ? { ...clone(srv), connected: true, last_connected: new Date().toISOString() } : null
  },
  async get_mcp_server_config(args: { name: string }) {
    await delay()
    const srv = MOCK_MCP_SERVERS.find(s => s.name === args.name)
    return srv ? { name: srv.name, command: srv.command, args: [], env: {}, enabled: srv.enabled } : null
  },

  // --- Skills ---
  async list_skills() { await delay(); return clone(MOCK_SKILLS) },
  async get_skill_detail(args: { name: string }) {
    await delay()
    const s = MOCK_SKILLS.find(x => x.name === args.name)
    return s ? { ...clone(s), content: `# ${s.name}\n\nSkill template body...`, parameters: [] } : null
  },

  // --- Plugins ---
  async list_plugins() { await delay(); return clone(MOCK_PLUGINS) },
  async install_plugin() { await delay(800); return 'plugin-installed' },
  async install_plugin_from_git() { await delay(1200); return 'plugin-installed-git' },
  async uninstall_plugin() { await delay() },
  async enable_plugin() { await delay() },
  async disable_plugin() { await delay() },
  async update_plugin() { await delay() },
  async list_plugin_marketplace() { await delay(); return [] },

  // --- Background Tasks ---
  async start_background_task(args: { prompt: string }) {
    await delay(100)
    const id = `bg-${Date.now()}`
    state.background.unshift({
      task_id: id,
      prompt: args.prompt,
      status: 'running',
      started_at: Date.now(),
      completed_at: null,
      output: 'Starting...',
    })
    return id
  },
  async get_background_tasks() { await delay(); return clone(state.background) },
  async cancel_background_task() { await delay(); return true },

  // --- Agents ---
  async list_agents() { await delay(); return clone(MOCK_AGENTS) },
  async list_agent_messages() { await delay(); return [] },
  async list_agent_message_teams() { await delay(); return ['product', 'engineering', 'growth'] },
  async record_agent_message() { await delay(20); return `msg-${Date.now()}` },
  async list_agent_definitions() { await delay(); return clone(MOCK_AGENT_DEFINITIONS) },
  async create_agent_definition(args: { name: string }) {
    await delay(200)
    return `agent-${args.name}-${Date.now()}`
  },
  async delete_agent_definition() { await delay(); return true },

  // --- Tasks ---
  async list_tasks() { await delay(); return clone(state.tasks) },
  async update_task(args: { payload: { id: string; status?: string; assignee?: string; priority?: string } }) {
    await delay(80)
    const t = findTask(args.payload.id)
    if (!t) throw new Error(`Task ${args.payload.id} not found`)
    if (args.payload.status) t.status = args.payload.status
    if (args.payload.assignee) t.assignee = args.payload.assignee
    if (args.payload.priority) t.priority = args.payload.priority
    return clone(t)
  },
  async get_task_detail(args: { id: string }) {
    await delay()
    const t = findTask(args.id)
    if (!t) throw new Error(`Task ${args.id} not found`)
    return clone(t)
  },

  // --- Scheduled ---
  async list_scheduled_tasks() { await delay(); return clone(state.scheduled) },
  async create_scheduled_task(args: { payload: { name: string } }) {
    await delay(200)
    const r = clone(MOCK_SCHEDULED_ROUTINES[0])
    r.id = `sched-${Date.now()}`
    r.name = args.payload.name
    state.scheduled.unshift(r)
    return r
  },
  async update_scheduled_task(args: { payload: { id: string } }) {
    await delay(100)
    const r = state.scheduled.find(x => x.id === args.payload.id)
    return r ? clone(r) : null
  },
  async delete_scheduled_task() { await delay(60); return true },
  async toggle_scheduled_task(args: { id: string; enabled: boolean }) {
    await delay(60)
    const r = state.scheduled.find(x => x.id === args.id)
    if (r) (r as { enabled: boolean }).enabled = args.enabled
    return r ? clone(r) : null
  },
  async trigger_task_now() {
    await delay(200)
    return { triggered: true, message: 'Task triggered. Result will appear shortly.' }
  },
  async preview_cron(args: { expr: string }) {
    await delay(40)
    return {
      expr: args.expr,
      valid: true,
      next_runs: ['Mon 9:00am', 'Tue 9:00am', 'Wed 9:00am'],
      human: 'Every day at 9:00am',
    }
  },

  // --- Inbox (P0-3 SQLite inbox) ---
  async list_inbox_items(args: { status?: string | null; source?: string | null; limit?: number | null }) {
    await delay()
    return clone(
      state.inbox
        .filter(i => (args.status ? i.status === args.status : true))
        .filter(i => (args.source ? i.source === args.source : true))
        .sort((a, b) => b.createdAtMs - a.createdAtMs)
        .slice(0, args.limit ?? 100),
    )
  },
  async update_inbox_item_status(args: { id: number; status: InboxItem['status'] }) {
    await delay(40)
    const item = state.inbox.find(i => i.id === args.id)
    if (!item) throw new Error(`inbox item not found: ${args.id}`)
    item.status = args.status
    item.updatedAtMs = Date.now()
    return undefined
  },
  async get_inbox_stats() {
    await delay()
    const startOfToday = new Date()
    startOfToday.setHours(0, 0, 0, 0)
    return {
      pending: state.inbox.filter(i => i.status === 'pending').length,
      today: state.inbox.filter(i => i.createdAtMs >= startOfToday.getTime()).length,
    }
  },
  async rerun_inbox_item(args: { id: number }) {
    await delay(200)
    const item = state.inbox.find(i => i.id === args.id)
    if (!item) throw new Error(`inbox item not found: ${args.id}`)
    if (item.source === 'goal' || item.source === 'trigger') {
      throw new Error(`inbox item source '${item.source}' cannot be rerun`)
    }
    // Simulate the unattended run: it completes a moment later and lands a
    // fresh pending item in the demo inbox (the real backend emits
    // `inbox-updated` when this happens).
    setTimeout(() => {
      state.inbox.unshift({
        id: nextInboxId++,
        source: item.source,
        sourceId: item.sourceId,
        sessionId: `sess-${String(nextInboxId).padStart(3, '0')}`,
        title: `${item.title} (rerun)`,
        summary: 'Rerun finished successfully.',
        error: null,
        status: 'pending',
        createdAtMs: Date.now(),
        updatedAtMs: Date.now(),
      })
    }, 1500)
    return `run-${Date.now()}`
  },
  async continue_inbox_item_session(args: { id: number }) {
    await delay(60)
    const item = state.inbox.find(i => i.id === args.id)
    if (!item) throw new Error(`inbox item not found: ${args.id}`)
    if (!item.sessionId) throw new Error(`inbox item ${args.id} has no linked session`)
    return item.sessionId
  },

  // --- Goal runs (P0-2 desktop goal runner) ---
  async list_goal_runs() {
    await delay()
    return clone(goalRuns).sort((a, b) => (b.startedAtMs as number) - (a.startedAtMs as number))
  },
  async get_goal_run(args: { sessionId: string }) {
    await delay()
    return clone(goalRuns.find(r => r.sessionId === args.sessionId) ?? null)
  },
  async start_goal_run(args: { sessionId?: string | null; title: string; objective: string; maxTurns?: number | null; budgetUsd?: number | null }) {
    await delay(120)
    const sessionId = args.sessionId ?? `0196goal-0000-7000-8000-${String(goalRuns.length + 1).padStart(12, '0')}`
    if (goalRuns.some(r => r.sessionId === sessionId && (r.status === 'running' || r.status === 'paused'))) {
      throw new Error('a goal run is already active on this session')
    }
    goalRuns.unshift({
      sessionId,
      title: args.title,
      objective: args.objective,
      status: 'running',
      iterations: 0,
      maxTurns: args.maxTurns ?? null,
      spentUsd: 0,
      budgetUsd: args.budgetUsd ?? null,
      stallStrikes: 0,
      lastError: null,
      startedAtMs: Date.now(),
      updatedAtMs: Date.now(),
    })
    return { sessionId }
  },
  async stop_goal_run(args: { sessionId: string }) {
    await delay(60)
    const run = goalRuns.find(r => r.sessionId === args.sessionId)
    if (run) { run.status = 'stopped'; run.updatedAtMs = Date.now() }
    return undefined
  },
  async pause_goal_run(args: { sessionId: string }) {
    await delay(60)
    const run = goalRuns.find(r => r.sessionId === args.sessionId)
    if (!run || run.status !== 'running') throw new Error('no running goal run for this session')
    run.status = 'paused'
    run.updatedAtMs = Date.now()
    return undefined
  },
  async resume_goal_run(args: { sessionId: string }) {
    await delay(60)
    const run = goalRuns.find(r => r.sessionId === args.sessionId)
    if (!run || run.status !== 'paused') throw new Error('no paused goal run for this session')
    run.status = 'running'
    run.updatedAtMs = Date.now()
    return undefined
  },
  async update_goal_objective(args: { sessionId: string; objective: string }) {
    await delay(60)
    const run = goalRuns.find(r => r.sessionId === args.sessionId)
    if (!run) throw new Error('no goal found for this session')
    run.objective = args.objective
    run.updatedAtMs = Date.now()
    return undefined
  },

  // --- History ---
  async list_task_executions() {
    await delay()
    return Array.from({ length: 8 }).map((_, i) => ({
      id: `exec-${1000 - i}`,
      task_id: MOCK_SCHEDULED_ROUTINES[i % MOCK_SCHEDULED_ROUTINES.length].id,
      task_name: MOCK_SCHEDULED_ROUTINES[i % MOCK_SCHEDULED_ROUTINES.length].name,
      started_at: Math.floor((Date.now() - i * 86400_000) / 1000),
      completed_at: Math.floor((Date.now() - i * 86400_000 + 600) / 1000),
      status: i === 0 ? 'failed' : 'succeeded',
      duration_secs: 600,
      output_preview: 'Task output preview...',
    }))
  },
  async get_execution_detail(args: { id: string }) {
    await delay()
    return {
      id: args.id,
      task_id: 'sched-001',
      task_name: 'weekly-metrics-digest',
      started_at: Math.floor((Date.now() - 86400_000) / 1000),
      completed_at: Math.floor((Date.now() - 86400_000 + 612) / 1000),
      status: 'succeeded',
      duration_secs: 612,
      output: 'Full task output here...\nLine 2\nLine 3',
    }
  },

  // --- Triggered routines ---
  async list_triggered_routines() { await delay(); return clone(MOCK_TRIGGERED_ROUTINES) },
  async toggle_triggered_routine() { await delay(40); return true },
  async create_triggered_routine(args: { name: string; trigger: string; command: string }) {
    await delay(120)
    return {
      name: args.name,
      trigger: args.trigger,
      command: args.command,
      matcher: '',
      pattern: '',
      description: '',
      enabled: true,
      last_fired_at: null,
      fire_count: 0,
    }
  },

  // --- Hook events + profiles ---
  async list_hook_events() { await delay(); return clone(MOCK_HOOK_EVENTS) },
  async list_permission_profiles() { await delay(); return clone(MOCK_PROFILES) },
  // P1-3: frozen contract — activate_permission_profile(name: string|null).
  async activate_permission_profile(args: { name: string | null }) {
    await delay(60)
    const name = (args?.name ?? '').trim()
    if (name !== '' && name !== 'strict' && name !== 'balanced' && name !== 'permissive' &&
        !MOCK_PROFILES.custom.some((c) => c.name === name)) {
      throw new Error(`unknown permission profile \`${name}\``)
    }
    demoConfig.active_permission_profile = name === '' ? null : name
    // Mirror the backend's mode mapping so the demo header reflects it.
    if (name === 'strict' || name === 'balanced') demoConfig.approval_mode = 'suggest'
    else if (name === 'permissive') demoConfig.approval_mode = 'auto_edit'
    return { active: name === '' ? null : name, approval_mode: demoConfig.approval_mode }
  },
  async save_custom_profile(args: { name: string; description?: string; auto_approve: string[]; confirm: string[]; deny: string[] }) {
    await delay(100)
    const trimmed = args.name.trim()
    if (trimmed === '') throw new Error('profile name must not be empty')
    const row = {
      name: trimmed,
      description: args.description ?? '',
      auto_approve: args.auto_approve,
      confirm: args.confirm,
      deny: args.deny,
    }
    const existing = MOCK_PROFILES.custom.findIndex((c) => c.name === trimmed)
    if (existing >= 0) MOCK_PROFILES.custom[existing] = row
    else MOCK_PROFILES.custom.push(row)
    return clone(row)
  },
  async delete_custom_profile(args: { name: string }) {
    await delay(60)
    const idx = MOCK_PROFILES.custom.findIndex((c) => c.name === args?.name)
    if (idx >= 0) MOCK_PROFILES.custom.splice(idx, 1)
    if (demoConfig.active_permission_profile === args?.name) {
      demoConfig.active_permission_profile = null
    }
    return idx >= 0 ? [`.shannon/profiles/${args.name}.toml`] : []
  },

  // --- OPC analytics ---
  async get_opc_metrics() { await delay(); return clone(MOCK_OPC_METRICS) },


  // --- File context ---
  async get_file_context() {
    await delay()
    return [
      { path: 'src/billing/invoice.rs', name: 'invoice.rs', language: 'rust', lines: 312, relevant_lines: [{ start: 42, end: 60 }] },
      { path: 'src/webhooks/stripe.rs', name: 'stripe.rs', language: 'rust', lines: 184 },
    ]
  },

  // --- LSP ---
  async lsp_code_actions() { await delay(120); return { actions: clone(MOCK_CODE_ACTIONS) } },
  async apply_code_action() { await delay(100); return 1 },
  async read_source_file(args: { path: string }) {
    await delay()
    return {
      path: args.path,
      content: `// Source for ${args.path}\n\nfn main() {\n    println!("hello");\n}\n`,
      language_id: 'rust',
    }
  },

  // --- Memory ---
  async list_memories(args?: { project?: string | null; category?: string | null; query?: string | null }) {
    await delay()
    const all = MOCK_MEMORIES
    return clone(all.filter(m => {
      if (args?.project && m.project !== args.project) return false
      if (args?.category && m.category !== args.category) return false
      if (args?.query) {
        const q = args.query.toLowerCase()
        return m.content.toLowerCase().includes(q) || m.tags.some(t => t.toLowerCase().includes(q))
      }
      return true
    }))
  },
  async list_memory_projects() { await delay(); return clone(MOCK_MEMORY_PROJECTS) },
  async get_memory_stats() { await delay(); return clone(MOCK_MEMORY_STATS) },
  async create_memory(args: { project: string; category: string; content: string; tags?: string[] }) {
    await delay()
    return {
      id: `mem-${Date.now()}`,
      project: args.project,
      category: args.category,
      content: args.content,
      tags: args.tags ?? [],
      confidence: 0.8,
      created_at: new Date().toISOString(),
      accessed_at: new Date().toISOString(),
      access_count: 0,
    }
  },
  async update_memory() { await delay() },
  async delete_memory() { await delay() },
  async search_memories(args: { query: string; project?: string | null }) {
    return handlers.list_memories({ query: args.query, project: args.project })
  },

  // --- Notification preferences (Notifications P2 DND / quiet hours) ---
  async get_notification_prefs() {
    await delay()
    return clone(notificationPrefs)
  },
  async set_notification_prefs(args: { prefs: { master_enabled: boolean; dnd_enabled: boolean; dnd_start: string | null; dnd_end: string | null; on_completed: boolean; on_failed: boolean } }) {
    await delay()
    notificationPrefs = { ...args.prefs }
  },

  // --- Extensions Hub: Featured ---
  async list_featured_vendors() { await delay(); return clone(MOCK_FEATURED_VENDORS) },

  // --- Extensions Hub: Skill / Agent catalogs (B1-B3 from design review) ---
  async list_skill_catalog() { await delay(); return clone(MOCK_SKILL_CATALOG) },
  async list_installed_skill_plugins() { await delay(); return clone(MOCK_INSTALLED_SKILLS) },
  async uninstall_skill_plugin() { await delay(60); return undefined },
  async install_skill_from_repo() { await delay(800); return { success: true, message: 'Skill installed (mock)' } },
  async install_native_skill() { await delay(400); return { success: true, message: 'Skill installed (mock)' } },

  async list_agent_catalog() { await delay(); return clone(MOCK_AGENT_CATALOG) },
  async list_installed_agent_plugins() { await delay(); return clone(MOCK_INSTALLED_AGENTS) },
  async uninstall_agent_plugin() { await delay(60); return undefined },
  async install_agent_from_repo() { await delay(800); return { success: true, message: 'Agent installed (mock)' } },
  async install_native_agent() { await delay(400); return { success: true, message: 'Agent installed (mock)' } },

  async list_installed_addons() { await delay(); return clone(MOCK_INSTALLED_ADDONS) },

  // --- Batch runs (P1-2 desktop best-of-N) ---
  async list_batch_runs() {
    await delay()
    return clone(batchRuns).sort((a, b) => (b.createdAtMs as number) - (a.createdAtMs as number))
  },
  async start_batch_run(args: { title: string; prompt: string; count: number }) {
    await delay()
    const batchId = `0196batch-0000-7000-8000-${String(batchRuns.length + 3).padStart(12, '0')}`
    const count = Math.min(4, Math.max(2, args.count))
    batchRuns.unshift({
      batchId,
      title: args.title.trim() || args.prompt.slice(0, 50),
      prompt: args.prompt,
      count,
      status: 'running',
      createdAtMs: Date.now(),
      branches: Array.from({ length: count }, (_, i) => demoBatchBranch(i, 'running', 0, 0)),
      adoptedIndex: null,
    })
    // Demo "progress": branches finish one by one.
    setTimeout(() => {
      const run = batchRuns.find(r => r.batchId === batchId)
      if (!run) return
      const branches = run.branches as ReturnType<typeof demoBatchBranch>[]
      branches.forEach((b, i) => {
        setTimeout(() => {
          if (i === branches.length - 1 && branches.length > 2) {
            b.status = 'failed'
            b.error = 'provider overloaded (429)'
          } else {
            b.status = 'completed'
          }
          b.summary = { filesChanged: 2 + i, additions: (2 + i) * 7, deletions: (2 + i) * 2 }
          b.spentUsd = 0.1 + 0.09 * i
          if (branches.every(x => x.status !== 'running')) run.status = branches.some(x => x.status === 'failed') ? 'partially_failed' : 'completed'
        }, 2500 * (i + 1))
      })
    }, 1500)
    return { batchId }
  },
  async get_batch_branch_diff(args: { batchId: string; index: number }) {
    await delay()
    return { diff: demoPatch(args.index) }
  },
  async adopt_batch_branch(args: { batchId: string; index: number }) {
    await delay()
    const run = batchRuns.find(r => r.batchId === args.batchId)
    if (!run) throw new Error(`batch not found: ${args.batchId}`)
    if (run.status === 'running') throw new Error('batch is still running — wait for all branches to finish before adopting')
    const conflict = (run.branches as ReturnType<typeof demoBatchBranch>[]).length > 2 && args.index === 2
    if (conflict) {
      return { merged: false, conflicts: ['src/search.ts'] }
    }
    run.status = 'adopted'
    run.adoptedIndex = args.index
    return { merged: true, conflicts: null }
  },
  // --- Live preview (P1-5 C-1) ---
  async preview_detect() {
    await delay()
    return {
      devServer: demoPreview.running
        ? null
        : { command: 'npm run dev', url: PREVIEW_URL },
    }
  },
  async preview_start() {
    await delay(500)
    demoPreview.running = true
    demoPreview.url = PREVIEW_URL
    demoPreview.startedAtMs = Date.now()
    return { url: PREVIEW_URL }
  },
  async preview_stop() {
    await delay()
    demoPreview.running = false
    demoPreview.url = null
    demoPreview.startedAtMs = null
  },
  async preview_status() {
    await delay()
    return clone(demoPreview)
  },
  async preview_capture() {
    await delay()
    return { imageBase64: PREVIEW_PNG, mediaType: 'image/png', width: 1, height: 1 }
  },
  async preview_logs() {
    await delay()
    return demoPreview.running
      ? [
          { tsMs: demoPreview.startedAtMs ?? Date.now(), stream: 'system', text: 'starting `npm run dev`' },
          { tsMs: Date.now(), stream: 'stdout', text: 'VITE v6.0.1  ready in 231 ms' },
          { tsMs: Date.now(), stream: 'stdout', text: `Local: ${PREVIEW_URL}/` },
        ]
      : []
  },

  async discard_batch_run(args: { batchId: string }) {
    await delay()
    const idx = batchRuns.findIndex(r => r.batchId === args.batchId)
    if (idx < 0) throw new Error(`batch not found: ${args.batchId}`)
    const run = batchRuns[idx]
    if (run.status === 'adopted') throw new Error('batch was already adopted — nothing to discard')
    const running = (run.branches as ReturnType<typeof demoBatchBranch>[]).filter(b => b.status === 'running').length
    batchRuns.splice(idx, 1)
    return { removed: (run.count as number) - running, skipped: running > 0 ? [`branch: still running`] : [] }
  },
}

export const mockDiagnostics = MOCK_DIAGNOSTICS
export const mockPerfTraces = MOCK_PERF_TRACES
export const mockGoals = MOCK_GOALS
