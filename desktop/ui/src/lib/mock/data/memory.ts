// Memory + Featured Vendors mock data.
import type { MemoryEntry, MemoryStats, FeaturedVendor } from '@/lib/tauri-api'

export const MOCK_MEMORIES: MemoryEntry[] = [
  {
    id: 'mem-001',
    project: 'my-startup',
    category: 'preference',
    content: 'Prefers concise responses without hedging language.',
    tags: ['style', 'tone'],
    confidence: 0.92,
    created_at: '2026-06-20T10:15:00Z',
    accessed_at: '2026-06-24T14:32:00Z',
    access_count: 7,
  },
  {
    id: 'mem-002',
    project: 'my-startup',
    category: 'decision',
    content: 'Use Postgres for billing — SQLite was rejected due to concurrent write contention.',
    tags: ['db', 'billing'],
    confidence: 0.85,
    created_at: '2026-06-18T09:20:00Z',
    accessed_at: '2026-06-22T11:05:00Z',
    access_count: 3,
  },
  {
    id: 'mem-003',
    project: 'my-startup',
    category: 'pattern',
    content: 'User typically starts morning with triage review, then deep-work block 9–12.',
    tags: ['workflow', 'schedule'],
    confidence: 0.78,
    created_at: '2026-06-22T08:00:00Z',
    accessed_at: '2026-06-24T08:15:00Z',
    access_count: 5,
  },
]

export const MOCK_MEMORY_PROJECTS: string[] = ['my-startup', 'personal', 'research']

export const MOCK_MEMORY_STATS: MemoryStats = {
  total: 3,
  by_category: { preference: 1, decision: 1, pattern: 1 },
  by_project: { 'my-startup': 3 },
  most_recent_at: '2026-06-22T08:00:00Z',
}

export const MOCK_FEATURED_VENDORS: FeaturedVendor[] = [
  {
    slug: 'notion',
    display_name: 'Notion',
    description: 'Search and edit your Notion workspace from Shannon.',
    icon: 'description',
    category: 'productivity',
    trust: 'official',
    install_kind: {
      type: 'oauth_remote',
      authorize_url: 'https://api.notion.com/v1/oauth/authorize',
      token_url: 'https://api.notion.com/v1/oauth/token',
      mcp_endpoint: 'https://mcp.notion.com/v1',
      client_id_env: 'NOTION_OAUTH_CLIENT_ID',
      default_scopes: ['read', 'write'],
      display_name: 'Notion MCP',
    },
    homepage_url: 'https://notion.so',
  },
  {
    slug: 'github',
    display_name: 'GitHub',
    description: 'Repo, issues, and PRs from the GitHub MCP server.',
    icon: 'code',
    category: 'developer_tools',
    trust: 'verified',
    install_kind: {
      type: 'stdio',
      command: 'npx',
      args: ['-y', '@modelcontextprotocol/server-github'],
      env_vars: [['GITHUB_TOKEN', '']],
      display_name: 'GitHub MCP (stdio)',
    },
    homepage_url: 'https://github.com',
  },
  {
    slug: 'slack',
    display_name: 'Slack',
    description: 'Send messages and search channels via Slack MCP.',
    icon: 'forum',
    category: 'communication',
    trust: 'verified',
    install_kind: {
      type: 'oauth_remote',
      authorize_url: 'https://slack.com/oauth/v2/authorize',
      token_url: 'https://slack.com/api/oauth.v2.access',
      mcp_endpoint: 'https://mcp.slack.com/v1',
      client_id_env: 'SLACK_OAUTH_CLIENT_ID',
      default_scopes: ['chat:write', 'channels:read'],
      display_name: 'Slack MCP',
    },
    homepage_url: 'https://slack.com',
  },
]
