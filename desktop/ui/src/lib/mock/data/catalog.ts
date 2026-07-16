import type {
  SkillCatalogEntry,
  AgentCatalogEntry,
  InstalledSkill,
  InstalledAgent,
} from '@/lib/tauri-api'

export const MOCK_SKILL_CATALOG: SkillCatalogEntry[] = [
  {
    id: 'skill-brainstorm',
    kind: 'skill',
    name: 'brainstorm',
    description: 'Socratic discovery mindset — probe requirements before writing code.',
    author: 'anthropics',
    version: '0.3.1',
    homepage_url: 'https://github.com/anthropics/skills',
    license: 'MIT',
    stars: 842,
    last_updated: '2026-06-12',
    source: { type: 'git_hub_repo', repo: 'anthropics/skills', ref_: 'main' },
    trust: 'official',
    metadata: { category: 'workflow' },
    tags: ['discovery', 'planning', 'socratic'],
  },
  {
    id: 'skill-tdd',
    kind: 'skill',
    name: 'tdd',
    description: 'Test-driven development workflow with strict red/green/refactor gates.',
    author: 'obra',
    version: '1.2.0',
    homepage_url: 'https://github.com/obra/superpowers',
    license: 'Apache-2.0',
    stars: 412,
    last_updated: '2026-06-08',
    source: { type: 'git_hub_repo', repo: 'obra/superpowers', ref_: 'main' },
    trust: 'verified',
    metadata: { category: 'workflow' },
    tags: ['testing', 'red-green', 'discipline'],
  },
  {
    id: 'skill-systematic-debug',
    kind: 'skill',
    name: 'systematic-debugging',
    description: 'Hypothesis-driven root cause analysis — never skip the diagnosis phase.',
    author: 'obra',
    version: '1.1.3',
    homepage_url: 'https://github.com/obra/superpowers',
    license: 'Apache-2.0',
    stars: 367,
    last_updated: '2026-06-05',
    source: { type: 'git_hub_repo', repo: 'obra/superpowers', ref_: 'main' },
    trust: 'verified',
    metadata: { category: 'workflow' },
    tags: ['debugging', 'hypothesis', 'root-cause'],
  },
  {
    id: 'skill-frontend-design',
    kind: 'skill',
    name: 'frontend-design',
    description: 'Modern UI component generation from 21st.dev patterns with design system integration.',
    author: 'community',
    version: '0.4.0',
    homepage_url: null,
    license: 'MIT',
    stars: 156,
    last_updated: '2026-05-28',
    source: { type: 'custom', url: 'https://example.com/skills/frontend-design' },
    trust: 'community',
    metadata: { category: 'frontend' },
    tags: ['react', 'tailwind', 'design-system'],
  },
]

export const MOCK_AGENT_CATALOG: AgentCatalogEntry[] = [
  {
    id: 'agent-code-reviewer',
    kind: 'agent',
    name: 'code-reviewer',
    description: 'Separate-pass reviewer for security, performance, and architectural issues.',
    author: 'anthropics',
    version: '0.5.2',
    homepage_url: 'https://github.com/anthropics/skills',
    license: 'MIT',
    stars: 612,
    last_updated: '2026-06-10',
    source: { type: 'git_hub_repo', repo: 'anthropics/skills' },
    trust: 'official',
    metadata: {
      trigger: 'on PR open',
      model: 'opus',
      tools: ['read', 'grep', 'git_diff'],
      system_prompt: 'You are a senior code reviewer focused on...',
    },
    tags: ['review', 'security', 'quality'],
  },
  {
    id: 'agent-executor',
    kind: 'agent',
    name: 'executor',
    description: 'Implementation agent for multi-file changes — follows plans strictly.',
    author: 'obra',
    version: '1.4.0',
    homepage_url: 'https://github.com/obra/superpowers',
    license: 'Apache-2.0',
    stars: 489,
    last_updated: '2026-06-07',
    source: { type: 'git_hub_repo', repo: 'obra/superpowers' },
    trust: 'verified',
    metadata: {
      trigger: 'on task assign',
      model: 'sonnet',
      tools: ['edit', 'write', 'bash', 'read'],
    },
    tags: ['implementation', 'executor', 'coder'],
  },
  {
    id: 'agent-verifier',
    kind: 'agent',
    name: 'verifier',
    description: 'Independent verification agent — never approves work it didn\'t check.',
    author: 'obra',
    version: '1.3.1',
    homepage_url: 'https://github.com/obra/superpowers',
    license: 'Apache-2.0',
    stars: 305,
    last_updated: '2026-06-03',
    source: { type: 'git_hub_repo', repo: 'obra/superpowers' },
    trust: 'verified',
    metadata: {
      trigger: 'post-implementation',
      model: 'sonnet',
      tools: ['bash', 'read', 'grep', 'test_runner'],
    },
    tags: ['verification', 'testing', 'qa'],
  },
]

export const MOCK_INSTALLED_SKILLS: InstalledSkill[] = [
  {
    name: 'brainstorm',
    path: '~/.shannon/skills/brainstorm.md',
    installed_at: '2026-06-15T10:30:00Z',
  },
  {
    name: 'tdd',
    path: '~/.shannon/skills/tdd.md',
    installed_at: '2026-06-15T10:31:00Z',
  },
]

export const MOCK_INSTALLED_AGENTS: InstalledAgent[] = [
  {
    name: 'executor',
    path: '~/.shannon/agents/executor.md',
    installed_at: '2026-06-14T08:15:00Z',
  },
  {
    name: 'code-reviewer',
    path: '~/.shannon/agents/code-reviewer.md',
    installed_at: '2026-06-14T08:16:00Z',
  },
]

export interface InstalledAddonInfo {
  kind: 'mcp_server' | 'skill' | 'agent' | 'plugin' | 'datasource'
  name: string
  version: string | null
  installed_at: string | null
  source: string
  enabled: boolean
}

export const MOCK_INSTALLED_ADDONS: InstalledAddonInfo[] = [
  { kind: 'mcp_server', name: 'filesystem', version: '1.0.0', installed_at: '2026-06-01T09:00:00Z', source: 'official', enabled: true },
  { kind: 'mcp_server', name: 'github', version: '2.3.1', installed_at: '2026-06-02T14:30:00Z', source: 'verified', enabled: true },
  { kind: 'mcp_server', name: 'playwright', version: '0.4.0', installed_at: '2026-06-03T11:15:00Z', source: 'verified', enabled: false },
  { kind: 'skill', name: 'brainstorm', version: '0.3.1', installed_at: '2026-06-15T10:30:00Z', source: 'anthropics/skills', enabled: true },
  { kind: 'skill', name: 'tdd', version: '1.2.0', installed_at: '2026-06-15T10:31:00Z', source: 'obra/superpowers', enabled: true },
  { kind: 'agent', name: 'executor', version: '1.4.0', installed_at: '2026-06-14T08:15:00Z', source: 'obra/superpowers', enabled: true },
  { kind: 'agent', name: 'code-reviewer', version: '0.5.2', installed_at: '2026-06-14T08:16:00Z', source: 'anthropics/skills', enabled: true },
]
