// Configuration mock data: desktop config, models, status, working dir.
import type {
  DesktopConfig,
  ModelInfo,
  ProviderProfileSummary,
  ProvidersFile,
  StatusResponse,
  ToolInfo,
} from '@/types'

export const MOCK_CONFIG: DesktopConfig = {
  provider: 'anthropic',
  api_key: 'sk-ant-•••••••••••••••••',
  base_url: 'https://api.anthropic.com',
  model: 'claude-sonnet-4-6',
  working_dir: '/Users/demo/workspace/my-startup',
  theme: 'system',
  approval_mode: 'standard',
  version: __APP_VERSION__,
  strategic_focus: 'Build a consumer AI agent desktop',
  performance_strategy: 'balanced',
  memory_enabled: true,
  telemetry_enabled: false,
  encryption_enabled: true,
  debug_console: false,
  temperature: 0.7,
  max_tokens: 8192,
  plan: 'Pro',
  // P1-3: execution mode + command sandbox defaults for the demo.
  active_permission_profile: 'balanced',
  sandbox: { mode: 'off' },
  // P2-5: off-peak model override (frozen key `offpeak.model_override`).
  // Empty = disabled in the demo.
  offpeak: { model_override: '' },
  // R3-3: plan/act phase tiers — null = inherit (no phase override).
  plan_tier: null,
  act_tier: null,
  // B2: real sub-agent execution — off in the demo (no live bridge).
  agent_teams_enabled: false,
  // Settings R3 T3: hardware acceleration on, always-on keep-awake off,
  // run-time sleep blocker on — the backend defaults, verbatim.
  hardware_acceleration: true,
  power_keep_awake: false,
  power_block_sleep_during_tasks: true,
  // Settings R3 T4 (B1): corporate-network trio unset — the demo keeps the
  // implicit env fallback (R1), never a forced proxy or CA.
  network_proxy_url: null,
  network_no_proxy: null,
  network_ca_cert_path: null,
  // Settings R3 T6: auto-compaction on — the backend default, verbatim.
  context_auto_compact: true,
  // Settings R3 T7: timed auto-archive off with 7-day retention — the
  // backend serde defaults, verbatim.
  session_auto_archive_enabled: false,
  session_auto_archive_days: 7,
  // Settings R3 T8: 提问自动继续 off — the backend default, verbatim (the
  // agent waits for the user's answer indefinitely).
  chat_ask_user_auto_continue: false,
}

// Managed-providers roster for the Models P2 UI (mirrors the Rust
// ~/.shannon/desktop/providers.json store). Seeded with the active Anthropic
// connection plus a second OpenAI-compatible entry so the demo roster isn't
// empty and the Activate/Edit/Delete flows are exercisable.
export const MOCK_PROVIDERS: ProvidersFile = {
  active_provider_id: 'prov-anthropic',
  providers: [
    {
      id: 'prov-anthropic',
      display_name: 'Anthropic',
      kind: 'anthropic',
      has_api_key: true,
      base_url: null,
    },
    {
      id: 'prov-glm',
      display_name: 'GLM (Zhipu)',
      kind: 'openai-compatible',
      has_api_key: true,
      base_url: 'https://open.bigmodel.cn/api/paas/v4',
    },
  ],
}

export const MOCK_MODELS: ModelInfo[] = [
  // R2-3: prices + vision ride along so the composer picker's context/price/
  // capability enrichment is exercisable in demo mode and e2e. Unknown stays
  // represented as null (renders "—"). R3-3: the catalog `tier` label rides
  // along too — the real backend populates it from the same classification
  // the plan/act tier controls resolve with.
  { id: 'claude-opus-4-7', name: 'Claude Opus 4.7', provider: 'anthropic', context_window: 200_000, price_in: 15, price_out: 75, vision: true, tier: 'pro' },
  { id: 'claude-sonnet-4-6', name: 'Claude Sonnet 4.6', provider: 'anthropic', context_window: 200_000, price_in: 3, price_out: 15, vision: true, tier: 'standard' },
  { id: 'claude-haiku-4-5-20251001', name: 'Claude Haiku 4.5', provider: 'anthropic', context_window: 200_000, price_in: 0.8, price_out: 4, vision: false, tier: 'fast' },
  { id: 'gpt-5', name: 'GPT-5', provider: 'openai', context_window: 256_000, price_in: 1.25, price_out: 10, vision: true, tier: 'pro' },
  { id: 'gpt-5-mini', name: 'GPT-5 Mini', provider: 'openai', context_window: 128_000, price_in: 0.25, price_out: 2, tier: 'fast' },
  { id: 'gemini-3-pro', name: 'Gemini 3 Pro', provider: 'google', context_window: 2_000_000, price_in: 1.25, price_out: 10, vision: true, tier: 'pro' },
  { id: 'llama-4-70b', name: 'Llama 4 70B (local)', provider: 'ollama', context_window: 32_000, price_in: 0, price_out: 0, vision: false, tier: 'standard' },
]

// R3-2: demo model-profile roster (mirrors the engine providers.toml v2
// `profiles` map — name, provider slots, active pointer). "default" mirrors
// the MOCK_PROVIDERS roster; the second profile exists so the switch flow
// (and its refresh of provider status + catalog) is exercisable in demo/e2e.
export const MOCK_PROVIDER_PROFILES: ProviderProfileSummary[] = [
  { name: 'default', provider_count: 2, active: true, model: 'claude-sonnet-4-6' },
  { name: 'research', provider_count: 1, active: false, model: 'gemini-3-pro' },
]

export const MOCK_STATUS: StatusResponse = {
  model: 'claude-sonnet-4-6',
  provider: 'anthropic',
  querying: false,
  message_count: 14,
  working_dir: '/Users/demo/workspace/my-startup',
}

export const MOCK_TOOLS: ToolInfo[] = [
  { name: 'read_file', description: 'Read a file from disk', enabled: true, read_only: true },
  { name: 'write_file', description: 'Write content to a file', enabled: true, read_only: false },
  { name: 'edit_file', description: 'Apply a structured edit to a file', enabled: true, read_only: false },
  { name: 'bash', description: 'Execute a shell command', enabled: true, read_only: false },
  { name: 'search', description: 'Search across files using ripgrep', enabled: true, read_only: true },
  { name: 'web_search', description: 'Search the web', enabled: true, read_only: true },
  { name: 'web_fetch', description: 'Fetch a URL and extract content', enabled: true, read_only: true },
  { name: 'send_email', description: 'Send an email via SMTP', enabled: true, read_only: false },
  { name: 'git_commit', description: 'Create a git commit', enabled: true, read_only: false },
  { name: 'git_diff', description: 'Show git diff', enabled: true, read_only: true },
  { name: 'mcp_invoke', description: 'Call an MCP server tool', enabled: true, read_only: false },
  { name: 'create_task', description: 'Create a task in the task system', enabled: true, read_only: false },
  { name: 'update_task', description: 'Update an existing task', enabled: true, read_only: false },
  { name: 'spawn_agent', description: 'Spawn a sub-agent', enabled: true, read_only: false },
  { name: 'computer_use', description: 'Click / type / screenshot', enabled: false, read_only: false },
]
