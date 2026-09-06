/**
 * P1-5 C-2 — workspace layout model (pure functions, zero dependencies).
 *
 * The grid is 12×12 cells: 12 CSS grid columns (brief) × 12 equal `1fr`
 * rows, so a panel's footprint is a plain rect and all geometry (drag,
 * resize, move, swap) is integer math here. Layouts persist per project
 * through the frozen `workspace_get_layout` / `workspace_set_layout` Tauri
 * commands (see `desktop/src/workspace_commands.rs`).
 *
 * Frozen decisions (task-11 report):
 *  - `projectKey` is computed FRONTEND-side from the session working
 *    directory (`workspaceProjectKey`: FNV-1a over the normalized path).
 *  - `version` mismatch (or any structural invalidity: unknown kind, rect
 *    out of grid, overlapping panels, missing/duplicated chat) resets to the
 *    default focus preset.
 *  - Panel kinds are exactly `chat | diff | preview | terminal`, one panel
 *    per kind (the terminal panel shares the drawer's TerminalManager, the
 *    preview panel the backend PreviewManager singleton — duplicates would
 *    be meaningless).
 */

/** Wire/ persistence version — must match workspace_commands.rs. */
export const WORKSPACE_LAYOUT_VERSION = 1

/** Grid geometry (matches GRID_COLUMNS/GRID_ROWS in workspace_commands.rs). */
export const GRID_COLUMNS = 12
export const GRID_ROWS = 12

export type PanelKind = 'chat' | 'diff' | 'preview' | 'terminal'

export const PANEL_KINDS: readonly PanelKind[] = ['chat', 'diff', 'preview', 'terminal']

/** 1-based col/row, span-count w/h. */
export interface PanelRect {
  col: number
  row: number
  w: number
  h: number
}

export interface PanelLayout {
  /** Stable within a layout; presets use the kind as the id. */
  id: string
  kind: PanelKind
  rect: PanelRect
}

export interface WorkspaceLayout {
  version: number
  panels: PanelLayout[]
}

export type PresetName = 'focus' | 'review' | 'build'
export type MoveDirection = 'up' | 'down' | 'left' | 'right'

// ── Rect math ────────────────────────────────────────────────────────────

export function rectsOverlap(a: PanelRect, b: PanelRect): boolean {
  return a.col < b.col + b.w && b.col < a.col + a.w && a.row < b.row + b.h && b.row < a.row + a.h
}

function isInt(n: unknown): n is number {
  return typeof n === 'number' && Number.isInteger(n)
}

/** A rect lies inside the 12×12 grid and every field is a positive int. */
export function rectFits(rect: PanelRect): boolean {
  return (
    isInt(rect.col) && isInt(rect.row) && isInt(rect.w) && isInt(rect.h) &&
    rect.col >= 1 && rect.row >= 1 && rect.w >= 1 && rect.h >= 1 &&
    rect.col + rect.w - 1 <= GRID_COLUMNS &&
    rect.row + rect.h - 1 <= GRID_ROWS
  )
}

function clampToGrid(rect: PanelRect): PanelRect {
  return {
    ...rect,
    w: Math.min(Math.max(1, rect.w), GRID_COLUMNS - rect.col + 1),
    h: Math.min(Math.max(1, rect.h), GRID_ROWS - rect.row + 1),
  }
}

function collides(panels: PanelLayout[], id: string, rect: PanelRect): boolean {
  return panels.some(p => p.id !== id && rectsOverlap(p.rect, rect))
}

/** A rect can be placed as-is: inside the grid and not overlapping. */
function canPlace(panels: PanelLayout[], id: string, rect: PanelRect): boolean {
  return rectFits(rect) && !collides(panels, id, rect)
}

// ── Presets (brief: 三套预设；空白区允许存在) ─────────────────────────────

/** Default sizes used when adding a panel to an existing layout. */
const ADD_DEFAULT_SIZE: Record<Exclude<PanelKind, 'chat'>, { w: number; h: number }> = {
  diff: { w: 4, h: 12 },
  preview: { w: 4, h: 12 },
  terminal: { w: 12, h: 3 },
}

/**
 * The three presets. `focus` (default) must look near-identical to the
 * pre-workspace chat page: a single chat panel covering the whole grid.
 */
export const PRESETS: Record<PresetName, WorkspaceLayout> = {
  focus: {
    version: WORKSPACE_LAYOUT_VERSION,
    panels: [{ id: 'chat', kind: 'chat', rect: { col: 1, row: 1, w: 12, h: 12 } }],
  },
  review: {
    version: WORKSPACE_LAYOUT_VERSION,
    panels: [
      { id: 'chat', kind: 'chat', rect: { col: 1, row: 1, w: 8, h: 12 } },
      { id: 'diff', kind: 'diff', rect: { col: 9, row: 1, w: 4, h: 12 } },
    ],
  },
  build: {
    version: WORKSPACE_LAYOUT_VERSION,
    panels: [
      { id: 'chat', kind: 'chat', rect: { col: 1, row: 1, w: 8, h: 8 } },
      { id: 'preview', kind: 'preview', rect: { col: 9, row: 1, w: 4, h: 8 } },
      { id: 'terminal', kind: 'terminal', rect: { col: 1, row: 9, w: 12, h: 4 } },
    ],
  },
}

/** Fresh deep copy of a preset (callers mutate layouts freely). */
export function presetLayout(name: PresetName): WorkspaceLayout {
  return {
    version: WORKSPACE_LAYOUT_VERSION,
    panels: PRESETS[name].panels.map(p => ({ ...p, rect: { ...p.rect } })),
  }
}

function sameRect(a: PanelRect, b: PanelRect): boolean {
  return a.col === b.col && a.row === b.row && a.w === b.w && a.h === b.h
}

/** True when the layout is exactly one of the named presets. */
export function matchesPreset(layout: WorkspaceLayout, name: PresetName): boolean {
  const preset = PRESETS[name]
  if (layout.version !== preset.version || layout.panels.length !== preset.panels.length) return false
  return preset.panels.every(p => {
    const actual = layout.panels.find(q => q.id === p.id)
    return actual !== undefined && actual.kind === p.kind && sameRect(actual.rect, p.rect)
  })
}

// ── Normalization / restore semantics ────────────────────────────────────

/**
 * Validate an untrusted layout (persisted JSON). Returns a normalized copy,
 * or `null` when the layout must be reset to the default preset:
 * version mismatch, unknown/duplicated kind, missing chat, rect out of the
 * grid, or overlapping panels.
 */
export function normalizeLayout(raw: unknown): WorkspaceLayout | null {
  if (typeof raw !== 'object' || raw === null) return null
  const candidate = raw as Partial<WorkspaceLayout>
  if (candidate.version !== WORKSPACE_LAYOUT_VERSION) return null
  if (!Array.isArray(candidate.panels)) return null

  const panels: PanelLayout[] = []
  const seenIds = new Set<string>()
  const seenKinds = new Set<PanelKind>()
  for (const entry of candidate.panels) {
    if (typeof entry !== 'object' || entry === null) return null
    const panel = entry as Partial<PanelLayout>
    if (typeof panel.id !== 'string' || panel.id.trim() === '') return null
    if (!PANEL_KINDS.includes(panel.kind as PanelKind)) return null
    if (seenIds.has(panel.id) || seenKinds.has(panel.kind as PanelKind)) return null
    const rect = panel.rect
    if (typeof rect !== 'object' || rect === null || !rectFits(rect as PanelRect)) return null
    seenIds.add(panel.id)
    seenKinds.add(panel.kind as PanelKind)
    panels.push({ id: panel.id, kind: panel.kind as PanelKind, rect: { ...(rect as PanelRect) } })
  }
  // The chat panel is the workspace's reason to exist — a saved layout
  // without one would leave the user with no way back to the conversation.
  if (!seenKinds.has('chat')) return null

  for (let i = 0; i < panels.length; i++) {
    for (let j = i + 1; j < panels.length; j++) {
      if (rectsOverlap(panels[i].rect, panels[j].rect)) return null
    }
  }
  return { version: WORKSPACE_LAYOUT_VERSION, panels }
}

/**
 * Restore semantics (brief): no saved layout → default focus preset;
 * version mismatch or invalid layout → reset to the default preset.
 */
export function resolveLayout(saved: unknown): WorkspaceLayout {
  return normalizeLayout(saved) ?? presetLayout('focus')
}

// ── Layout operations (all pure; return the SAME layout when blocked) ────

export function findPanel(layout: WorkspaceLayout, id: string): PanelLayout | undefined {
  return layout.panels.find(p => p.id === id)
}

/**
 * Place `id` at `rect` (clamped to the grid). Returns `null` when the panel
 * does not exist or the clamped rect would overlap another panel.
 */
export function withRect(layout: WorkspaceLayout, id: string, rect: PanelRect): WorkspaceLayout | null {
  const panel = findPanel(layout, id)
  if (!panel) return null
  const clamped = clampToGrid(rect)
  if (collides(layout.panels, id, clamped)) return null
  return {
    ...layout,
    panels: layout.panels.map(p => (p.id === id ? { ...p, rect: clamped } : p)),
  }
}

/**
 * Keyboard move (菜单「上移/下移/左移/右移」— the a11y-equivalent of drag).
 * One cell per step; blocked moves are no-ops (never overlap, never leave
 * the grid, never shrink the panel).
 */
export function movePanel(layout: WorkspaceLayout, id: string, dir: MoveDirection): WorkspaceLayout {
  const panel = findPanel(layout, id)
  if (!panel) return layout
  const delta = dir === 'left' ? { col: -1, row: 0 }
    : dir === 'right' ? { col: 1, row: 0 }
    : dir === 'up' ? { col: 0, row: -1 }
    : { col: 0, row: 1 }
  const target = {
    col: panel.rect.col + delta.col,
    row: panel.rect.row + delta.row,
    w: panel.rect.w,
    h: panel.rect.h,
  }
  if (!canPlace(layout.panels, id, target)) return layout
  return { ...layout, panels: layout.panels.map(p => (p.id === id ? { ...p, rect: target } : p)) }
}

/** Title-bar drag/drop equivalent: exchange the two panels' rects. */
export function swapPanels(layout: WorkspaceLayout, idA: string, idB: string): WorkspaceLayout {
  const a = findPanel(layout, idA)
  const b = findPanel(layout, idB)
  if (!a || !b || idA === idB) return layout
  return {
    ...layout,
    panels: layout.panels.map(p =>
      p.id === idA ? { ...p, rect: { ...b.rect } } : p.id === idB ? { ...p, rect: { ...a.rect } } : p,
    ),
  }
}

/**
 * Resize from the top-left anchor: shrink the requested size (w first,
 * then h) until it fits the grid without overlapping. Never returns null —
 * the original layout comes back when nothing fits.
 */
export function resizePanel(layout: WorkspaceLayout, id: string, rect: PanelRect): WorkspaceLayout {
  const panel = findPanel(layout, id)
  if (!panel) return layout
  const anchor = { col: panel.rect.col, row: panel.rect.row }
  const maxW = GRID_COLUMNS - anchor.col + 1
  const maxH = GRID_ROWS - anchor.row + 1
  const wantW = Math.min(Math.max(1, rect.w), maxW)
  const wantH = Math.min(Math.max(1, rect.h), maxH)
  for (let h = wantH; h >= 1; h--) {
    for (let w = wantW; w >= 1; w--) {
      const next = withRect(layout, id, { ...anchor, w, h })
      if (next) return next
    }
  }
  return layout
}

/**
 * Add a panel of `kind` (chat cannot be added — the workspace always owns
 * exactly one). First-fit scan over the grid with the kind's default size,
 * shrinking the request when needed; when the grid is already fully tiled
 * (e.g. the focus preset) the chat panel — the only one we ever shrink —
 * gives way to a per-kind sensible split. Idempotent per kind; returns the
 * layout unchanged when nothing fits.
 */
export function addPanel(layout: WorkspaceLayout, kind: PanelKind): WorkspaceLayout {
  if (kind === 'chat') return layout
  if (layout.panels.some(p => p.kind === kind)) return layout
  const def = ADD_DEFAULT_SIZE[kind]

  const direct = firstFit(layout, kind, def, true)
  if (direct) return direct

  const chat = layout.panels.find(p => p.kind === 'chat')
  if (!chat) return layout
  for (const chatRect of CHAT_SHRINK_CANDIDATES[kind]) {
    const shrunk = withRect(layout, chat.id, chatRect)
    if (!shrunk) continue
    const placed = firstFit(shrunk, kind, def, true)
    if (placed) return placed
  }
  return layout
}

/** Chat rects to try (in order) when a new panel needs the chat to yield. */
const CHAT_SHRINK_CANDIDATES: Record<Exclude<PanelKind, 'chat'>, PanelRect[]> = {
  diff: [{ col: 1, row: 1, w: 8, h: 12 }, { col: 1, row: 1, w: 12, h: 9 }, { col: 1, row: 1, w: 8, h: 8 }],
  preview: [{ col: 1, row: 1, w: 8, h: 12 }, { col: 1, row: 1, w: 12, h: 9 }, { col: 1, row: 1, w: 8, h: 8 }],
  terminal: [{ col: 1, row: 1, w: 12, h: 9 }, { col: 1, row: 1, w: 8, h: 12 }, { col: 1, row: 1, w: 8, h: 8 }],
}

/** Row-major first-fit scan; `allowShrink` also tries smaller sizes. */
function firstFit(
  layout: WorkspaceLayout,
  kind: Exclude<PanelKind, 'chat'>,
  def: { w: number; h: number },
  allowShrink: boolean,
): WorkspaceLayout | null {
  const minW = allowShrink ? 3 : def.w
  const minH = allowShrink ? 2 : def.h
  for (let h = def.h; h >= minH; h--) {
    for (let w = def.w; w >= minW; w--) {
      for (let row = 1; row <= GRID_ROWS - h + 1; row++) {
        for (let col = 1; col <= GRID_COLUMNS - w + 1; col++) {
          const rect = { col, row, w, h }
          if (canPlace(layout.panels, kind, rect)) {
            return { ...layout, panels: [...layout.panels, { id: kind, kind, rect }] }
          }
        }
      }
    }
  }
  return null
}

/** Remove a panel. The chat panel is not removable (no-op). */
export function removePanel(layout: WorkspaceLayout, id: string): WorkspaceLayout {
  const panel = findPanel(layout, id)
  if (!panel || panel.kind === 'chat') return layout
  return { ...layout, panels: layout.panels.filter(p => p.id !== id) }
}

// ── projectKey (frozen: computed frontend-side from the session cwd) ─────

/**
 * Deterministic per-project storage key from the session working directory.
 * FNV-1a (32-bit) over the normalized path + length suffix; the backend
 * treats the result as an opaque string. Normalization collapses `\` → `/`
 * and strips trailing separators so the same directory hashes identically.
 */
export function workspaceProjectKey(cwd: string): string {
  const normalized = cwd.trim().replace(/\\/g, '/').replace(/\/+$/, '')
  let hash = 0x811c9dc5
  for (let i = 0; i < normalized.length; i++) {
    hash ^= normalized.charCodeAt(i)
    hash = Math.imul(hash, 0x01000193) >>> 0
  }
  return `p-${hash.toString(16).padStart(8, '0')}-${normalized.length}`
}
