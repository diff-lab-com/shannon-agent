// Configuration mock data: desktop config, models, status, working dir.
import type {
  DesktopConfig,
  ModelInfo,
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
}

export const MOCK_MODELS: ModelInfo[] = [
  { id: 'claude-opus-4-7', name: 'Claude Opus 4.7', provider: 'anthropic', context_window: 200_000 },
  { id: 'claude-sonnet-4-6', name: 'Claude Sonnet 4.6', provider: 'anthropic', context_window: 200_000 },
  { id: 'claude-haiku-4-5-20251001', name: 'Claude Haiku 4.5', provider: 'anthropic', context_window: 200_000 },
  { id: 'gpt-5', name: 'GPT-5', provider: 'openai', context_window: 256_000 },
  { id: 'gpt-5-mini', name: 'GPT-5 Mini', provider: 'openai', context_window: 128_000 },
  { id: 'gemini-3-pro', name: 'Gemini 3 Pro', provider: 'google', context_window: 2_000_000 },
  { id: 'llama-4-70b', name: 'Llama 4 70B (local)', provider: 'ollama', context_window: 32_000 },
]

export const MOCK_STATUS: StatusResponse = {
  model: 'claude-sonnet-4-6',
  provider: 'anthropic',
  querying: false,
  message_count: 14,
  working_dir: '/Users/demo/workspace/my-startup',
}

export const MOCK_TOOLS: ToolInfo[] = [
  { name: 'read_file', description: 'Read a file from disk', enabled: true },
  { name: 'write_file', description: 'Write content to a file', enabled: true },
  { name: 'edit_file', description: 'Apply a structured edit to a file', enabled: true },
  { name: 'bash', description: 'Execute a shell command', enabled: true },
  { name: 'search', description: 'Search across files using ripgrep', enabled: true },
  { name: 'web_search', description: 'Search the web', enabled: true },
  { name: 'web_fetch', description: 'Fetch a URL and extract content', enabled: true },
  { name: 'send_email', description: 'Send an email via SMTP', enabled: true },
  { name: 'git_commit', description: 'Create a git commit', enabled: true },
  { name: 'git_diff', description: 'Show git diff', enabled: true },
  { name: 'mcp_invoke', description: 'Call an MCP server tool', enabled: true },
  { name: 'create_task', description: 'Create a task in the task system', enabled: true },
  { name: 'update_task', description: 'Update an existing task', enabled: true },
  { name: 'spawn_agent', description: 'Spawn a sub-agent', enabled: true },
  { name: 'computer_use', description: 'Click / type / screenshot', enabled: false },
]
