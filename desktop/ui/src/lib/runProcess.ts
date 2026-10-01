// runProcess — GB P2-3: the 「过程四要素」 aggregation state machine.
//
// While a streaming task runs, the chat's right dock gains a 运行 tab that
// aggregates, from the EXISTING query event stream only (no new backend
// state), the four things a user wants to see about the run:
//   1. 一句话进度摘要 — the latest tool-progress line (or the running tool)
//   2. 当前计划   — the engine's plan doc (Plan tab data, shown by RunPanel)
//   3. 引用来源   — @references + attachments from the send + tool-read files
//   4. 新产出文件 — files the run's write-like tools touched
//
// This module is the pure reducer: AppContext feeds it query events, the
// dock renders its snapshot. Lifecycle: beginRun() on send (clears the
// previous turn — content survives until the NEXT run starts), endRun() on
// completed/failed/cancelled, reset to idle on session switch.

/** Aggregated snapshot of the visible session's current/most recent run. */
export interface RunProcessState {
  status: 'idle' | 'running' | 'done' | 'failed'
  startedAt: number | null
  endedAt: number | null
  /** Sources the turn referenced: @refs from the message, attachment paths,
   *  and files the run's read-like tools opened. Insertion order, deduped. */
  sources: string[]
  /** Files the run's write-like tools created/modified. */
  outputs: string[]
  /** One-line progress: latest QUERY_TOOL_PROGRESS message, else the
   *  currently-running tool's name. */
  summary: string | null
  lastTool: string | null
  toolCount: number
}

export function initialRunProcess(): RunProcessState {
  return {
    status: 'idle',
    startedAt: null,
    endedAt: null,
    sources: [],
    outputs: [],
    summary: null,
    lastTool: null,
    toolCount: 0,
  }
}

/** Tool names whose job is CREATING/MODIFYING files (outputs). */
const WRITE_TOOL_RE = /(write|create|edit|apply|patch|save|insert|replace|move|rename)/i
/** Tool names whose job is READING/looking things up (sources). */
const READ_TOOL_RE = /(read|search|grep|glob|list|view|find|open|load|tree|fetch|web|http)/i

/** Extract up to `cap` path-looking strings from a tool's input object —
 *  string fields under path/file/dir-ish keys. Tolerates any shape. */
export function extractToolPaths(input: unknown, cap = 2): string[] {
  if (input == null || typeof input !== 'object') return []
  const out: string[] = []
  const visit = (node: unknown, depth: number) => {
    if (out.length >= cap || node == null || typeof node !== 'object' || depth > 3) return
    for (const [key, value] of Object.entries(node as Record<string, unknown>)) {
      if (out.length >= cap) return
      if (typeof value === 'string' && /path|file|dir/i.test(key) && value.length > 1 && value.length < 1024) {
        out.push(value)
      } else if (value != null && typeof value === 'object') {
        visit(value, depth + 1)
      }
    }
  }
  visit(input, 0)
  return out
}

/** @-references in the sent message text (same tokens the composer's
 *  mention menu inserts: whitespace-delimited, no @ inside). Dots are token
 *  characters (file names) — sentence-final punctuation is stripped. */
export function extractMessageRefs(message: string, cap = 8): string[] {
  if (!message) return []
  const out: string[] = []
  const re = /(?:^|\s)@([^\s@，。；、！？；：,!?：]+)/g
  let m: RegExpExecArray | null
  while ((m = re.exec(message)) !== null && out.length < cap) {
    const token = m[1].replace(/[.,;:]+$/, '')
    if (token) out.push(token)
  }
  return out
}

function pushUnique(list: string[], items: string[], cap: number): string[] {
  const next = [...list]
  for (const item of items) {
    if (!item || next.includes(item)) continue
    if (next.length >= cap) break
    next.push(item)
  }
  return next
}

const LIST_CAP = 50

/** A new turn starts: wipe the previous run and seed it with the send. */
export function beginRun(
  opts: { at: number; message?: string; attachments?: readonly string[] },
): RunProcessState {
  const refs = extractMessageRefs(opts.message ?? '')
  return {
    ...initialRunProcess(),
    status: 'running',
    startedAt: opts.at,
    sources: pushUnique([], [...refs, ...(opts.attachments ?? [])], LIST_CAP),
  }
}

/** A tool started — classify it into sources/outputs and refresh the summary. */
export function noteToolStart(
  prev: RunProcessState,
  toolName: string,
  toolInput: unknown,
  at: number,
): RunProcessState {
  const name = toolName ?? ''
  const paths = extractToolPaths(toolInput)
  let sources = prev.sources
  let outputs = prev.outputs
  if (WRITE_TOOL_RE.test(name)) {
    outputs = pushUnique(outputs, paths, LIST_CAP)
  } else if (READ_TOOL_RE.test(name) || paths.length > 0) {
    // Read-like, or unclassified but path-carrying: the conservative bucket
    // is "referenced", never "produced".
    sources = pushUnique(sources, paths, LIST_CAP)
  }
  return {
    ...prev,
    status: 'running',
    startedAt: prev.startedAt ?? at,
    sources,
    outputs,
    lastTool: name || prev.lastTool,
    toolCount: prev.toolCount + 1,
    summary: name ? name : prev.summary,
  }
}

/** Progress text supersedes the tool name in the one-line summary. */
export function noteToolProgress(prev: RunProcessState, message: string | undefined): RunProcessState {
  const text = message?.trim()
  if (!text || text === prev.summary) return prev
  return { ...prev, summary: text }
}

/** The run settled. Content persists until the next beginRun. */
export function endRun(prev: RunProcessState, at: number, failed: boolean): RunProcessState {
  if (prev.status !== 'running') return prev
  return { ...prev, status: failed ? 'failed' : 'done', endedAt: at }
}
