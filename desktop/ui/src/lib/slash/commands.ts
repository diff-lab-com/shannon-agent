// Slash-command registry — the desktop's counterpart to the REPL's command
// surface (crates/shannon-ui/src/repl/commands). One registry, two entry
// points: the chat composer autocomplete (primary) and the command palette
// for the self-contained actions.
//
// Scope rule: only commands the desktop can genuinely execute today. REPL
// commands that already have a dedicated desktop surface (extensions,
// memory, tasks, settings, model switching) map to navigation; session
// diagnostics (/context /cost /diff /export) call the Tauri backends;
// engine-side features the desktop doesn't expose yet (/compact) are
// deliberately absent until their backend lands.
//
// Parsing rule (see parseSlashInput): a command runs only when the ENTIRE
// trimmed input is `/name`. Anything else that starts with `/` — most
// importantly pasted absolute paths — goes to the model as plain text.

import * as api from '@/lib/tauri-api'
import { exportSessionAsMarkdown } from '@/lib/sessionActions'

export type { SessionContextStats, SessionUsageSummary, GitDiffFile, GitDiffSummary } from '@/lib/tauri-api'

export type SlashResult =
  | { kind: 'context'; stats: api.SessionContextStats }
  | { kind: 'cost'; usage: api.SessionUsageSummary }
  | { kind: 'diff'; diff: api.GitDiffSummary; workingDir: string }
  | { kind: 'error'; messageKey: string; values?: Record<string, string | number> }

export interface SlashCommandContext {
  navigate: (path: string) => void
  sessionId: string | null
  workingDir: string
  sessions: { id: string; title?: string | null }[]
  createSession: () => Promise<void>
  showResult: (result: SlashResult) => void
  toastError: (message: string, err: unknown) => void
  t: (id: string) => string
}

export interface SlashCommand {
  name: string
  aliases?: string[]
  icon: string
  labelKey: string
  descriptionKey: string
  /** Guarded with a "start a session first" notice when no chat is open. */
  needsSession?: boolean
  run: (ctx: SlashCommandContext) => void | Promise<void>
}

function requireSession(ctx: SlashCommandContext): string | null {
  if (!ctx.sessionId) {
    ctx.showResult({ kind: 'error', messageKey: 'slash.needsSession' })
    return null
  }
  return ctx.sessionId
}

export const SLASH_COMMANDS: SlashCommand[] = [
  {
    name: 'context',
    icon: 'data_usage',
    labelKey: 'slash.command.context.label',
    descriptionKey: 'slash.command.context.description',
    needsSession: true,
    run: async (ctx) => {
      const sessionId = requireSession(ctx)
      if (!sessionId) return
      try {
        ctx.showResult({ kind: 'context', stats: await api.getSessionContextStats(sessionId) })
      } catch (e) {
        ctx.toastError(ctx.t('slash.card.error.title'), e)
      }
    },
  },
  {
    name: 'cost',
    aliases: ['usage-session'],
    icon: 'payments',
    labelKey: 'slash.command.cost.label',
    descriptionKey: 'slash.command.cost.description',
    needsSession: true,
    run: async (ctx) => {
      const sessionId = requireSession(ctx)
      if (!sessionId) return
      try {
        ctx.showResult({ kind: 'cost', usage: await api.getSessionUsage(sessionId) })
      } catch (e) {
        ctx.toastError(ctx.t('slash.card.error.title'), e)
      }
    },
  },
  {
    name: 'diff',
    icon: 'difference',
    labelKey: 'slash.command.diff.label',
    descriptionKey: 'slash.command.diff.description',
    needsSession: true,
    run: async (ctx) => {
      const sessionId = requireSession(ctx)
      if (!sessionId) return
      if (!ctx.workingDir) {
        ctx.showResult({ kind: 'error', messageKey: 'slash.card.diff.noWorkingDir' })
        return
      }
      try {
        const diff = await api.getSessionGitDiff(ctx.workingDir)
        ctx.showResult({ kind: 'diff', diff, workingDir: ctx.workingDir })
      } catch (e) {
        ctx.toastError(ctx.t('slash.card.error.title'), e)
      }
    },
  },
  {
    name: 'export',
    aliases: ['save'],
    icon: 'download',
    labelKey: 'slash.command.export.label',
    descriptionKey: 'slash.command.export.description',
    needsSession: true,
    run: async (ctx) => {
      const sessionId = requireSession(ctx)
      if (!sessionId) return
      await exportSessionAsMarkdown(sessionId, ctx.sessions as never[], ctx.t)
    },
  },
  {
    name: 'new',
    aliases: ['clear'],
    icon: 'add_comment',
    labelKey: 'slash.command.new.label',
    descriptionKey: 'slash.command.new.description',
    run: (ctx) => {
      void ctx.createSession()
    },
  },
  { name: 'tasks', icon: 'task_alt', labelKey: 'nav.scheduled', descriptionKey: 'slash.command.tasks.description', run: (ctx) => ctx.navigate('/tasks') },
  { name: 'memory', icon: 'psychology', labelKey: 'nav.memory', descriptionKey: 'slash.command.memory.description', run: (ctx) => ctx.navigate('/memory') },
  { name: 'usage', icon: 'monitoring', labelKey: 'nav.usage', descriptionKey: 'slash.command.usage.description', run: (ctx) => ctx.navigate('/usage') },
  { name: 'extensions', aliases: ['agents'], icon: 'grid_view', labelKey: 'nav.extensions', descriptionKey: 'slash.command.extensions.description', run: (ctx) => ctx.navigate('/extensions') },
  { name: 'editor', icon: 'code', labelKey: 'nav.editor', descriptionKey: 'slash.command.editor.description', run: (ctx) => ctx.navigate('/editor') },
  { name: 'settings', icon: 'settings', labelKey: 'nav.settings', descriptionKey: 'slash.command.settings.description', run: (ctx) => ctx.navigate('/settings') },
]

/** `/name` or partial `/na` — single token only, so pasted paths never trip it. */
const SLASH_QUERY_RE = /^\/([A-Za-z][\w-]*)?$/

/** True while the composer should show the autocomplete menu. */
export function isSlashQuery(input: string): boolean {
  return SLASH_QUERY_RE.test(input.trimEnd())
}

/**
 * Resolve a fully typed input to a command. Only a bare `/name` counts —
 * `/name args` and unknown `/tokens` (likely paths) return null and are
 * sent as text.
 */
export function parseSlashInput(input: string): SlashCommand | null {
  const trimmed = input.trim()
  if (!SLASH_QUERY_RE.test(trimmed)) return null
  const name = trimmed.slice(1).toLowerCase()
  return SLASH_COMMANDS.find(cmd => cmd.name === name || cmd.aliases?.includes(name)) ?? null
}

/** Prefix/substring filter for the autocomplete menu. */
export function filterSlashCommands(query: string): SlashCommand[] {
  const name = query.replace(/^\//, '').toLowerCase()
  if (!name) return SLASH_COMMANDS
  return SLASH_COMMANDS.filter(cmd =>
    cmd.name.startsWith(name) ||
    cmd.aliases?.some(a => a.startsWith(name)) ||
    cmd.name.includes(name),
  )
}
