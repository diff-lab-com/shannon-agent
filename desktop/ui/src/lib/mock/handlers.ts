// Mock command handlers — map every Tauri command used by tauri-api.ts to a mock response.
// Handlers are async to mimic network latency. All return clones so consumers can't mutate the data.
import { MOCK_TASKS, MOCK_AGENTS, MOCK_AGENT_DEFINITIONS, MOCK_SESSIONS, MOCK_MESSAGES,
  MOCK_SKILLS, MOCK_MCP_SERVERS, MOCK_PLUGINS, MOCK_BACKGROUND_TASKS } from './data/core'
import { MOCK_SCHEDULED_ROUTINES, MOCK_TRIGGERED_ROUTINES, MOCK_HOOK_EVENTS, MOCK_PROFILES } from './data/automation'
import { MOCK_TRIAGE_ITEMS, MOCK_TRIAGE_STATS, MOCK_OPC_METRICS, MOCK_BILLING_PLAN,
  MOCK_COST_HISTORY, MOCK_BILLING_HISTORY, MOCK_PERF_TRACES, MOCK_DIAGNOSTICS,
  MOCK_CODE_ACTIONS, MOCK_GOALS } from './data/analytics'
import { MOCK_CONFIG, MOCK_MODELS, MOCK_STATUS, MOCK_TOOLS, MOCK_PROVIDERS } from './data/config'
import type { ProviderInput } from '@/types'
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

// Mutable state for "live" feeling during demo
const state = {
  tasks: clone(MOCK_TASKS),
  scheduled: clone(MOCK_SCHEDULED_ROUTINES),
  background: clone(MOCK_BACKGROUND_TASKS),
  providers: clone(MOCK_PROVIDERS),
}

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
  async get_config() { await delay(); return clone(MOCK_CONFIG) },
  async configure() { await delay() },
  async switch_provider() { await delay(200) },

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
        label: input.label,
        provider_kind: input.provider_kind,
        api_key: keepKey ? existing.api_key : input.api_key,
        base_url: input.base_url || null,
        model: input.model || null,
      })
    } else {
      state.providers.providers.push({
        id: `prov-${Date.now()}`,
        label: input.label,
        provider_kind: input.provider_kind,
        api_key: input.api_key || null,
        base_url: input.base_url || null,
        model: input.model || null,
        created_at: new Date().toISOString(),
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
  async new_session() { await delay(60); return `sess-${Date.now()}` },
  async list_sessions() { await delay(); return clone(MOCK_SESSIONS) },
  async search_sessions(args: { query: string }) {
    await delay()
    const q = (args.query ?? '').toLowerCase()
    return clone(MOCK_SESSIONS.filter(s => s.title.toLowerCase().includes(q)))
  },
  async load_session() { await delay(); return clone(MOCK_MESSAGES) },
  async switch_session() { await delay(); return clone(MOCK_MESSAGES) },
  async delete_session() { await delay(60); return true },
  async rename_session() { await delay(60); return true },
  async duplicate_session(args: { id: string }) {
    await delay(60)
    const src = MOCK_SESSIONS.find(s => s.id === args.id)
    return src ? { ...clone(src), id: `sess-${Date.now()}`, title: `${src.title} copy` } : null
  },
  async export_session() { await delay(120); return '# Exported session\n\n(mock content)' },

  // --- Permissions ---
  async respond_permission() { await delay(20) },

  // --- Files ---
  async get_file_diff(args: { path: string }) {
    await delay()
    return {
      old_content: `// old content of ${args.path}\nfn main() { println!(\"hi\"); }`,
      new_content: `// new content of ${args.path}\nfn main() { println!(\"hello world\"); }`,
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

  // --- Triage ---
  async list_triage_items() { await delay(); return clone(MOCK_TRIAGE_ITEMS) },
  async mark_triage_read(args: { id: string }) {
    await delay(40)
    const item = MOCK_TRIAGE_ITEMS.find(i => i.id === args.id)
    if (item) (item as { read: boolean }).read = true
    return true
  },
  async archive_triage_item(args: { id: string }) {
    await delay(40)
    const item = MOCK_TRIAGE_ITEMS.find(i => i.id === args.id)
    if (item) (item as { archived: boolean }).archived = true
    return true
  },
  async get_triage_stats() { await delay(); return clone(MOCK_TRIAGE_STATS) },

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
  async save_custom_profile(args: { name: string }) {
    await delay(100)
    return {
      name: args.name,
      description: '',
      auto_approve: [],
      confirm: [],
      deny: [],
    }
  },
  async delete_custom_profile() { await delay(60); return ['standard', 'relaxed', 'strict'] },

  // --- OPC analytics ---
  async get_opc_metrics() { await delay(); return clone(MOCK_OPC_METRICS) },

  // --- Billing ---
  async get_billing_plan() { await delay(); return clone(MOCK_BILLING_PLAN) },
  async get_cost_history(args: { days: number }) {
    await delay()
    return clone(MOCK_COST_HISTORY.slice(-Math.min(args.days ?? 14, 14)))
  },
  async get_billing_history() { await delay(); return clone(MOCK_BILLING_HISTORY) },

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
      content: `// Source for ${args.path}\n\nfn main() {\n    println!(\"hello\");\n}\n`,
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
}

export const mockDiagnostics = MOCK_DIAGNOSTICS
export const mockPerfTraces = MOCK_PERF_TRACES
export const mockGoals = MOCK_GOALS
