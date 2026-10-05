// toolGrouping — Settings R3 T11 (C6): collapse runs of consecutive same-kind
// tool calls into foldable Explore / Terminal / Changes groups.
//
// Three concerns live here:
//   1. the read-only lookup map — seeded from `list_tools`' new `read_only`
//      field (`Tool::is_read_only()` backend-side); classification consults
//      the map first and only falls back to name heuristics when a tool is
//      unknown to the registry (e.g. history predating the field, MCP tools
//      from an unconnected server);
//   2. `classifyTool` — the Explore (read-only) / Terminal (non-read-only
//      shell) / Changes (file-mutating) decision for ONE tool name;
//   3. `groupToolSegments` — the pure sequence→segments projection used by
//      MessageBubble: adjacent same-kind cards merge into a group segment
//      ONLY while that kind's switch is on; special cards (subagent blocks,
//      retry-chain members, …) are marked non-groupable by the caller and
//      always pass through individually, and the retry-chain banner rides
//      along as an ordered marker so the failure→retry narrative keeps its
//      original position.
//
// Streaming keeps per-card rendering by controller ruling R10 (grouping a
// live stream re-folds the tail every flush — jitter); groups render once
// the message commits to history.
//
// Persistence mirrors the lib/thinkingPref.ts localStorage pattern: three
// `shannon.chat.grouping.*` keys, all default ON.

import { getTools } from '@/lib/tauri-api'
import type { ToolInfo } from '@/types'
import { FILE_MUTATING_TOOLS } from '@/lib/fileRefs'

export type ToolGroupKind = 'explore' | 'terminal' | 'changes'

export const TOOL_GROUP_KINDS: readonly ToolGroupKind[] = ['explore', 'terminal', 'changes']

/** localStorage keys for the three per-kind switches (default ON). */
export const GROUPING_PREF_KEYS: Record<ToolGroupKind, string> = {
  explore: 'shannon.chat.grouping.explore',
  terminal: 'shannon.chat.grouping.terminal',
  changes: 'shannon.chat.grouping.changes',
}

/* ─────── 1. read-only lookup map ─────── */

/** tool name (raw AND separator-stripped keys) → `Tool::is_read_only()`. */
const readOnlyByName = new Map<string, boolean>()

let mapVersion = 0
const mapListeners = new Set<() => void>()
let mapRequested = false

function notifyMapListeners() {
  mapVersion++
  for (const fn of mapListeners) fn()
}

/** Seed the map from a `getTools()` payload (also the tests' seam). Raw
 *  names are stored as-is plus in normalized form so lookups survive
 *  casing/separator drift between engine tool names and wire names. */
export function setToolReadOnlyMap(tools: Pick<ToolInfo, 'name' | 'read_only'>[]): void {
  for (const t of tools) {
    readOnlyByName.set(t.name, t.read_only)
    readOnlyByName.set(normalizeToolName(t.name), t.read_only)
  }
  notifyMapListeners()
}

/** Fire-once lazy seed. Failures reset the latch so a later render retries
 *  (registry not ready at first paint, dev mock hiccup, …). */
export function ensureToolReadOnlyMap(): Promise<void> {
  if (mapRequested) return Promise.resolve()
  mapRequested = true
  return getTools()
    .then(tools => setToolReadOnlyMap(tools))
    .catch(() => {
      mapRequested = false
    })
}

/** useSyncExternalStore seam — bump on every map mutation. */
export function subscribeToolReadOnlyMap(onChange: () => void): () => void {
  mapListeners.add(onChange)
  return () => mapListeners.delete(onChange)
}

export function getToolReadOnlyMapVersion(): number {
  return mapVersion
}

/** Test seam: drop the map + subscription state. */
export function resetToolReadOnlyMap(): void {
  readOnlyByName.clear()
  mapRequested = false
  notifyMapListeners()
}

/* ─────── 2. classifyTool ─────── */

/** Lowercase and strip separators so 'Bash' / 'bash' / 'terminal_command'
 *  / 'terminal-command' all hit the same heuristic entry. */
export function normalizeToolName(name: string): string {
  return name.toLowerCase().replace(/[-_.\s]/g, '')
}

/** Non-read-only shell runners → the Terminal group. Built from the brief's
 *  seed list plus the harness's own shell surfaces (PowerShell, AppleScript,
 *  REPL, background run). Only consulted when the read-only map misses or
 *  confirms the tool is NOT read-only (a read-only shell — e.g. a sandboxed
 *  dry-run variant — stays in Explore). */
const SHELL_TOOL_NAMES = new Set(
  ['bash', 'shell', 'terminal_command', 'run_command', 'powershell', 'applescript', 'repl', 'run_background'].map(
    normalizeToolName,
  ),
)

/** Heuristic fallback for tools the map doesn't know: the existing
 *  fileRefs write set plus the harness's canonical mutating tool names
 *  (Write/Edit/MultiEdit/NotebookEdit/MergeResolve/write_xlsx), all
 *  normalized. Everything unknown lands in Explore. */
const FILE_MUTATING_NAMES = new Set(
  [
    ...FILE_MUTATING_TOOLS,
    'write',
    'edit',
    'multiedit',
    'notebookedit',
    'mergeresolve',
    'write_xlsx',
  ].map(normalizeToolName),
)

/**
 * Classify ONE tool call by name for grouping.
 *
 * 三桶语义边界 (review fix round 1): the read-only map is authoritative,
 * and its flag only separates "read-only" from "mutates state" — it cannot
 * tell FILE mutation from other side effects. So a map hit that is NOT
 * read-only lands in `terminal` for shell runners and in `changes` for
 * EVERYTHING else: browser_* / computer_use / schedule_wakeup / messaging
 * tools group under the 「文件更改 / File changes」 banner even though they
 * touch no files. That is the documented boundary (state mutation other
 * than terminal), not a bug — the UI copy stays 写文件类; revisiting it
 * would need a richer backend classification than `is_read_only`.
 *
 * Order: read-only map first (authoritative, from `list_tools`) →
 * name heuristics only when the map doesn't know the tool.
 *  - map hit, read-only            → explore
 *  - map hit, NOT read-only        → terminal for shell names, else changes
 *  - map miss                      → FILE_MUTATING set → changes;
 *                                    shell list → terminal; else explore
 */
export function classifyTool(toolName: string): ToolGroupKind {
  const key = normalizeToolName(toolName)
  const readOnly = readOnlyByName.get(toolName) ?? readOnlyByName.get(key)
  if (readOnly != null) {
    if (readOnly) return 'explore'
    return SHELL_TOOL_NAMES.has(key) ? 'terminal' : 'changes'
  }
  if (FILE_MUTATING_NAMES.has(key)) return 'changes'
  if (SHELL_TOOL_NAMES.has(key)) return 'terminal'
  return 'explore'
}

/* ─────── 3. grouping prefs ─────── */

export type GroupingPrefs = Record<ToolGroupKind, boolean>

/** Read the three switches. Missing/garbage values fall back to ON — the
 *  grouping is the default presentation. */
export function readGroupingPrefs(): GroupingPrefs {
  const prefs = { explore: true, terminal: true, changes: true }
  try {
    for (const kind of TOOL_GROUP_KINDS) {
      const raw = localStorage.getItem(GROUPING_PREF_KEYS[kind])
      if (raw !== null) prefs[kind] = raw !== 'false'
    }
  } catch {
    /* storage unavailable → defaults */
  }
  return prefs
}

export function writeGroupingPref(kind: ToolGroupKind, value: boolean): void {
  try {
    localStorage.setItem(GROUPING_PREF_KEYS[kind], value ? 'true' : 'false')
  } catch {
    /* noop */
  }
}

/* ─────── 4. sequence → segments ─────── */

/** One item of the caller's tool walk. `tool` units are grouping
 *  candidates (or forced singles via `groupable: false` — special cards
 *  such as subagent blocks and retry-chain members). `marker` units are
 *  ORDERED PASSTHROUGHS the caller renders verbatim (the retry-chain
 *  banner): they are emitted in sequence position, and crossing one
 *  reopens the segmentation — a run may never span a marker. */
export type ToolGroupUnit<T, M = unknown> =
  | { type: 'tool'; tc: T; groupable: boolean }
  | { type: 'marker'; marker: M }

export type ToolGroupSegment<T, M = unknown> =
  | { type: 'group'; kind: ToolGroupKind; items: T[] }
  | { type: 'single'; tc: T }
  | { type: 'marker'; marker: M }

/**
 * Project a tool-call sequence into render segments (pure — no React, no
 * storage reads beyond what `classifyTool` already saw).
 *
 * Rules:
 *  - adjacent groupable units of the SAME kind merge, but only while that
 *    kind's switch is ON and the run has ≥ 2 calls;
 *  - kind change, switch OFF, or a non-groupable unit cuts the run —
 *    everything cut renders as individual singles (original cards);
 *  - markers emit at their sequence position (fix round 1: the retry-chain
 *    banner must stay between its preceding card and its chain's first
 *    failed card, never hoisted to the top of the tool area) and reset
 *    the current run;
 *  - all-batch classification is stable regardless of the map's async
 *    arrival: callers re-render via the map version subscription.
 */
export function groupToolSegments<T extends { tool_name: string }, M>(
  units: ToolGroupUnit<T, M>[],
  prefs: GroupingPrefs,
): ToolGroupSegment<T, M>[] {
  const segments: ToolGroupSegment<T, M>[] = []
  let run: { kind: ToolGroupKind; items: T[] } | null = null

  const flush = () => {
    if (!run) return
    if (run.items.length >= 2 && prefs[run.kind]) {
      segments.push({ type: 'group', kind: run.kind, items: run.items })
    } else {
      for (const tc of run.items) segments.push({ type: 'single', tc })
    }
    run = null
  }

  for (const unit of units) {
    if (unit.type === 'marker') {
      flush()
      segments.push(unit)
      continue
    }
    const { tc, groupable } = unit
    const kind = classifyTool(tc.tool_name)
    if (groupable && prefs[kind]) {
      if (run && run.kind === kind) {
        run.items.push(tc)
        continue
      }
      flush()
      run = { kind, items: [tc] }
      continue
    }
    flush()
    segments.push({ type: 'single', tc })
  }
  flush()
  return segments
}
