// Standalone type module — the original @/components/workspace/layout
// was retired with the WorkspaceGrid component in commit e786ec25, but the
// Tauri command contracts (`workspace_get_layout` / `workspace_set_layout`)
// and the mock handler still reference the WorkspaceLayout shape. Until
// the backend wiring is removed, this keeps the type alive without
// pulling in the rest of the old workspace module.

export const WORKSPACE_LAYOUT_VERSION = 1
export const GRID_COLUMNS = 12
export const GRID_ROWS = 12

export interface PanelRect {
  col: number
  row: number
  w: number
  h: number
}

export interface PanelLayout {
  id: string
  kind: string
  rect: PanelRect
}

export interface WorkspaceLayout {
  version: number
  panels: PanelLayout[]
}
