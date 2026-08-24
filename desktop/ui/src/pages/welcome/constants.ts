// Static catalog + stepper label keys for the Welcome flow.
//
// Split out of Welcome.tsx (T3.1) so the page orchestrator stays focused on
// step state + handlers. The constants here are read-only at runtime — every
// i18n key resolves through `useIntl()` inside the rendering component.

// ─── Task taxonomy ──────────────────────────────────────────────────────────
// Drives Step 0 (primary use case). Each task carries a model recommendation
// and a tool preset surfaced in Step 2.
//
// `labelKey` / `blurbKey` resolve via react-intl; provider names and tool
// names are deliberately left untranslated (proper nouns).
export type TaskId = 'code' | 'writing' | 'research' | 'general'

export interface TaskOption {
  id: TaskId
  labelKey: string
  blurbKey: string
  icon: string
  recommendedProvider: string
  tools: string[]
}

export const TASKS: TaskOption[] = [
  {
    id: 'code',
    labelKey: 'welcome.task.code.label',
    blurbKey: 'welcome.task.code.blurb',
    icon: 'code',
    recommendedProvider: 'anthropic',
    tools: ['filesystem', 'git', 'playwright'],
  },
  {
    id: 'writing',
    labelKey: 'welcome.task.writing.label',
    blurbKey: 'welcome.task.writing.blurb',
    icon: 'edit_note',
    recommendedProvider: 'anthropic',
    tools: ['web_search'],
  },
  {
    id: 'research',
    labelKey: 'welcome.task.research.label',
    blurbKey: 'welcome.task.research.blurb',
    icon: 'search',
    recommendedProvider: 'openai',
    tools: ['web_search', 'tavily'],
  },
  {
    id: 'general',
    labelKey: 'welcome.task.general.label',
    blurbKey: 'welcome.task.general.blurb',
    icon: 'auto_awesome',
    recommendedProvider: 'anthropic',
    tools: ['filesystem', 'web_search'],
  },
]

export const PROVIDERS = [
  { id: 'anthropic', label: 'Anthropic', descKey: 'welcome.model.anthropic.desc' },
  { id: 'openai', label: 'OpenAI', descKey: 'welcome.model.openai.desc' },
  { id: 'ollama', label: 'Ollama', descKey: 'welcome.model.ollama.desc' },
  { id: 'deepseek', label: 'DeepSeek', descKey: 'welcome.model.deepseek.desc' },
] as const

export const TOOL_CATALOG: Record<string, { labelKey: string; icon: string; descKey: string }> = {
  filesystem: { labelKey: 'welcome.tools.filesystem.label', icon: 'folder', descKey: 'welcome.tools.filesystem.desc' },
  git: { labelKey: 'welcome.tools.git.label', icon: 'commit', descKey: 'welcome.tools.git.desc' },
  playwright: { labelKey: 'welcome.tools.playwright.label', icon: 'web', descKey: 'welcome.tools.playwright.desc' },
  web_search: { labelKey: 'welcome.tools.webSearch.label', icon: 'travel_explore', descKey: 'welcome.tools.webSearch.desc' },
  tavily: { labelKey: 'welcome.tools.tavily.label', icon: 'menu_book', descKey: 'welcome.tools.tavily.desc' },
}

export const SHORTCUT_ROWS = [
  { keys: () => `${formatShortcut('K')}`, actionKey: 'shortcuts.openPalette' },
  { keys: () => `${formatShortcut('N')}`, actionKey: 'shortcuts.newChat' },
  { keys: () => `${formatShortcut('1')} / ${formatShortcut('2')} / ${formatShortcut('3')}`, actionKey: 'shortcuts.jumpTabs' },
  { keys: () => '?', actionKey: 'shortcuts.showAll' },
  { keys: () => 'Esc', actionKey: 'shortcuts.cancel' },
] as const

// Display labels for the Stepper (Step 0..3). Order matches STEPS_INDEX.
export const STEP_LABEL_KEYS = [
  'welcome.step.task',
  'welcome.step.model',
  'welcome.step.tools',
  'welcome.step.done',
] as const

// ─── Documents skill recommendations (P2.4) ─────────────────────────────────
// Instead of building a Documents engine inside Shannon (Phase D's MVP), we
// surface host-side Documents skills the user can install with one click.
// Each entry maps to a GitHub repo that `install_skill_from_repo` clones into
// `~/.shannon/skills/`. The catalog is deliberately short — only the most
// universally useful Documents skills.
//
// AVAILABILITY GATE: the shannon-skills-docs repos are not yet published, so
// one-click install would fail for every new user on first run. The whole
// section is hidden until DOCUMENTS_SKILLS_AVAILABLE is flipped true (a real
// repo-existence probe can replace this once a backend check / open CSP path
// exists). See claudedocs/comprehensive-audit-2026-06-29.md P1-1.
export const DOCUMENTS_SKILLS_AVAILABLE = false

export interface DocumentsSkill {
  id: string
  labelKey: string
  descKey: string
  icon: string
  repo: string
  ref: string
}

export const DOCUMENTS_SKILLS: DocumentsSkill[] = [
  {
    id: 'pandoc-docx',
    labelKey: 'welcome.skills.pandoc.label',
    descKey: 'welcome.skills.pandoc.desc',
    icon: 'description',
    repo: 'shannon-agent/shannon-skills-docs',
    ref: 'main',
  },
  {
    id: 'python-docx',
    labelKey: 'welcome.skills.pydocx.label',
    descKey: 'welcome.skills.pydocx.desc',
    icon: 'data_object',
    repo: 'shannon-agent/shannon-skills-docs',
    ref: 'main',
  },
  {
    id: 'markdown-beautify',
    labelKey: 'welcome.skills.beautify.label',
    descKey: 'welcome.skills.beautify.desc',
    icon: 'auto_fix_high',
    repo: 'shannon-agent/shannon-skills-docs',
    ref: 'main',
  },
]

// Format-aware shortcut renderer (re-exported here so the page module doesn't
// import directly from `@/lib/platform`).
import { formatShortcut } from '@/lib/platform'