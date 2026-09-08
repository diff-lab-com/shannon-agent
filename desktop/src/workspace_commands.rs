//! P1-5 C-2 — draggable panel workspace: per-project layout persistence.
//!
//! Frozen contract (Tauri commands):
//! * `workspace_get_layout({ projectKey }) -> WorkspaceLayout | null`
//! * `workspace_set_layout({ projectKey, layout }) -> ()`
//!
//! Storage follows the desktop-config convention (`~/.shannon/desktop/`):
//! one JSON file, `workspace-layouts.json`, mapping `projectKey → layout`.
//! The `projectKey` is computed **frontend-side** from the session working
//! directory (see `desktop/ui/src/components/workspace/layout.ts`,
//! `workspaceProjectKey` — FNV-1a over the normalized path); the backend
//! treats it as an opaque string.
//!
//! Version semantics (frozen): the layout payload carries a `version`; the
//! backend accepts only `SUPPORTED_LAYOUT_VERSION`. A *stored* layout with
//! any other version is reported as absent (`workspace_get_layout` → `null`)
//! so the UI resets to the default preset — forward-compatible without a
//! migration path. A corrupt store file is treated the same way; the next
//! successful `workspace_set_layout` rewrites it cleanly.
//!
//! Validation here is structural only (version, panel kinds, rect bounds,
//! unique ids/kinds). Collision-free placement is the frontend's job — the
//! grid editor never produces overlapping rects, and hand-edited files with
//! overlaps degrade gracefully (frontend `normalizeLayout` resets them).

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

/// The only layout version this build reads or writes. Anything else stored
/// on disk is reported as absent → the UI resets to the default preset.
pub const SUPPORTED_LAYOUT_VERSION: u32 = 1;

/// 12-column CSS grid (brief: 12 列, self-implemented, zero new deps).
pub const GRID_COLUMNS: u32 = 12;
/// The grid is 12×12 cells; row heights are `1fr` so every cell is equal.
pub const GRID_ROWS: u32 = 12;

/// Panel kinds (brief: 明确不做新面板类型).
pub const PANEL_KINDS: [&str; 4] = ["chat", "diff", "preview", "terminal"];

// ── DTOs (wire shape: camelCase) ─────────────────────────────────────────

/// Panel footprint on the 12×12 grid. `col`/`row` are 1-based; `w`/`h` are
/// span counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspacePanelRect {
    pub col: u32,
    pub row: u32,
    pub w: u32,
    pub h: u32,
}

/// One panel in the layout: id (stable across saves), kind, footprint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspacePanelLayout {
    pub id: String,
    pub kind: String,
    pub rect: WorkspacePanelRect,
}

/// `WorkspaceLayout` — `{ panels: [...], version }` (frozen contract).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceLayout {
    pub version: u32,
    pub panels: Vec<WorkspacePanelLayout>,
}

/// On-disk file shape: `{ "layouts": { "<projectKey>": layout } }`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkspaceLayoutStore {
    #[serde(default)]
    pub layouts: BTreeMap<String, WorkspaceLayout>,
}

// ── Pure store logic (unit-tested against a tempdir) ─────────────────────

/// Structural validation (see module docs for the collision disclaimer).
pub fn validate_layout(layout: &WorkspaceLayout) -> Result<(), String> {
    if layout.version != SUPPORTED_LAYOUT_VERSION {
        return Err(format!(
            "unsupported workspace layout version {} (supported: {})",
            layout.version, SUPPORTED_LAYOUT_VERSION
        ));
    }
    let mut ids: HashSet<&str> = HashSet::new();
    let mut kinds: HashSet<&str> = HashSet::new();
    for panel in &layout.panels {
        if panel.id.trim().is_empty() {
            return Err("panel id must be non-empty".to_string());
        }
        if !ids.insert(panel.id.as_str()) {
            return Err(format!("duplicate panel id: {}", panel.id));
        }
        if !PANEL_KINDS.contains(&panel.kind.as_str()) {
            return Err(format!("unknown panel kind: {}", panel.kind));
        }
        if !kinds.insert(panel.kind.as_str()) {
            return Err(format!("duplicate panel kind: {}", panel.kind));
        }
        let r = &panel.rect;
        if r.col == 0 || r.row == 0 || r.w == 0 || r.h == 0 {
            return Err(format!("panel {} rect fields must be >= 1", panel.id));
        }
        if r.col.saturating_add(r.w).saturating_sub(1) > GRID_COLUMNS {
            return Err(format!(
                "panel {} exceeds the {} grid columns",
                panel.id, GRID_COLUMNS
            ));
        }
        if r.row.saturating_add(r.h).saturating_sub(1) > GRID_ROWS {
            return Err(format!(
                "panel {} exceeds the {} grid rows",
                panel.id, GRID_ROWS
            ));
        }
    }
    Ok(())
}

/// Read the store file → map. Missing file → empty; unreadable/corrupt file
/// → empty + warn (recovery: the next `workspace_set_layout` rewrites it).
pub fn load_store_at(path: &Path) -> BTreeMap<String, WorkspaceLayout> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return BTreeMap::new(),
        Err(e) => {
            tracing::warn!(error = %e, path = %path.display(), "workspace layouts file unreadable");
            return BTreeMap::new();
        }
    };
    match serde_json::from_str::<WorkspaceLayoutStore>(&text) {
        Ok(store) => store.layouts,
        Err(e) => {
            tracing::warn!(error = %e, path = %path.display(), "workspace layouts file corrupt — treating as empty");
            BTreeMap::new()
        }
    }
}

/// Write the store file (creating parent dirs), pretty-printed for
/// hand-inspectability — the same convention as `mobile-devices.json`.
pub fn save_store_at(
    path: &Path,
    layouts: &BTreeMap<String, WorkspaceLayout>,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
    }
    let store = WorkspaceLayoutStore {
        layouts: layouts.clone(),
    };
    let json =
        serde_json::to_string_pretty(&store).map_err(|e| format!("serialize layouts: {e}"))?;
    std::fs::write(path, json).map_err(|e| format!("write {}: {e}", path.display()))
}

/// Resolve a stored layout: `None` when absent **or version-mismatched**
/// (version reset → the UI falls back to the default preset).
pub fn get_layout_in(
    layouts: &BTreeMap<String, WorkspaceLayout>,
    project_key: &str,
) -> Option<WorkspaceLayout> {
    layouts
        .get(project_key)
        .filter(|layout| layout.version == SUPPORTED_LAYOUT_VERSION)
        .cloned()
}

/// Insert/overwrite a project layout after validation.
pub fn set_layout_in(
    layouts: &mut BTreeMap<String, WorkspaceLayout>,
    project_key: &str,
    layout: WorkspaceLayout,
) -> Result<(), String> {
    validate_layout(&layout)?;
    layouts.insert(project_key.to_string(), layout);
    Ok(())
}

// ── Tauri commands (frozen contract) ─────────────────────────────────────

fn storage_path() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("Cannot determine home directory")?;
    Ok(home
        .join(".shannon")
        .join("desktop")
        .join("workspace-layouts.json"))
}

/// `workspace_get_layout({ projectKey }) -> WorkspaceLayout | null`
#[tauri::command]
pub async fn workspace_get_layout(project_key: String) -> Result<Option<WorkspaceLayout>, String> {
    let path = storage_path()?;
    Ok(get_layout_in(&load_store_at(&path), &project_key))
}

/// `workspace_set_layout({ projectKey, layout })`
#[tauri::command]
pub async fn workspace_set_layout(
    project_key: String,
    layout: WorkspaceLayout,
) -> Result<(), String> {
    let path = storage_path()?;
    let mut layouts = load_store_at(&path);
    set_layout_in(&mut layouts, &project_key, layout)?;
    save_store_at(&path, &layouts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(col: u32, row: u32, w: u32, h: u32) -> WorkspacePanelRect {
        WorkspacePanelRect { col, row, w, h }
    }

    fn panel(id: &str, kind: &str, r: WorkspacePanelRect) -> WorkspacePanelLayout {
        WorkspacePanelLayout {
            id: id.to_string(),
            kind: kind.to_string(),
            rect: r,
        }
    }

    fn focus_layout() -> WorkspaceLayout {
        WorkspaceLayout {
            version: SUPPORTED_LAYOUT_VERSION,
            panels: vec![panel("chat", "chat", rect(1, 1, 12, 12))],
        }
    }

    fn write_file(path: &Path, contents: &str) {
        std::fs::write(path, contents).unwrap();
    }

    #[test]
    fn validate_accepts_the_default_focus_layout() {
        validate_layout(&focus_layout()).unwrap();
    }

    #[test]
    fn validate_rejects_unknown_version() {
        let mut layout = focus_layout();
        layout.version = 99;
        let err = validate_layout(&layout).unwrap_err();
        assert!(
            err.contains("unsupported workspace layout version 99"),
            "{err}"
        );
    }

    #[test]
    fn validate_rejects_unknown_kind_and_duplicates() {
        let mut layout = focus_layout();
        layout.panels[0].kind = "editor".into();
        assert!(validate_layout(&layout).is_err());

        let mut layout = focus_layout();
        layout.panels.push(panel("diff", "diff", rect(1, 1, 4, 4)));
        layout.panels.push(panel("diff2", "diff", rect(5, 1, 4, 4)));
        let err = validate_layout(&layout).unwrap_err();
        assert!(err.contains("duplicate panel kind"), "{err}");

        let mut layout = focus_layout();
        layout.panels.push(panel("chat", "diff", rect(1, 1, 4, 4)));
        assert!(validate_layout(&layout).is_err());
    }

    #[test]
    fn validate_rejects_out_of_grid_rects() {
        let mut layout = focus_layout();
        layout.panels[0].rect = rect(11, 1, 4, 12); // col 11 + w 4 - 1 > 12
        assert!(validate_layout(&layout).is_err());
        layout.panels[0].rect = rect(1, 12, 12, 4); // row 12 + h 4 - 1 > 12
        assert!(validate_layout(&layout).is_err());
        layout.panels[0].rect = rect(0, 1, 12, 12); // 1-based
        assert!(validate_layout(&layout).is_err());
    }

    #[test]
    fn roundtrip_set_get_over_tempdir() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("workspace-layouts.json");

        let mut layouts = load_store_at(&path);
        assert!(layouts.is_empty(), "missing file reads as empty");

        let mut review = focus_layout();
        review.panels.push(panel("diff", "diff", rect(9, 1, 4, 12)));
        set_layout_in(&mut layouts, "p-alpha", review.clone()).unwrap();
        set_layout_in(&mut layouts, "p-beta", focus_layout()).unwrap();
        save_store_at(&path, &layouts).unwrap();

        let reloaded = load_store_at(&path);
        assert_eq!(get_layout_in(&reloaded, "p-alpha"), Some(review));
        assert_eq!(get_layout_in(&reloaded, "p-beta"), Some(focus_layout()));
        assert_eq!(
            get_layout_in(&reloaded, "p-gamma"),
            None,
            "unknown key → None"
        );
    }

    #[test]
    fn version_mismatch_reads_as_absent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("workspace-layouts.json");
        // Hand-written future-version store (set_layout_in would reject it —
        // exactly the "written by a newer app version" scenario).
        write_file(
            &path,
            r#"{"layouts":{"p-x":{"version":2,"panels":[{"id":"chat","kind":"chat","rect":{"col":1,"row":1,"w":12,"h":12}}]}}}"#,
        );
        let layouts = load_store_at(&path);
        assert_eq!(layouts.len(), 1, "the entry exists on disk");
        assert_eq!(get_layout_in(&layouts, "p-x"), None, "version reset → None");
    }

    #[test]
    fn corrupt_file_reads_as_empty_and_next_write_recovers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("workspace-layouts.json");
        write_file(&path, "{{{ not json");

        let layouts = load_store_at(&path);
        assert!(layouts.is_empty(), "corrupt file → empty store");

        let mut layouts = layouts;
        set_layout_in(&mut layouts, "p-x", focus_layout()).unwrap();
        save_store_at(&path, &layouts).unwrap();
        assert_eq!(
            get_layout_in(&load_store_at(&path), "p-x"),
            Some(focus_layout())
        );
    }

    #[test]
    fn set_rejects_invalid_layout_and_leaves_store_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("workspace-layouts.json");
        let mut layouts = load_store_at(&path);
        set_layout_in(&mut layouts, "p-ok", focus_layout()).unwrap();

        let mut bad = focus_layout();
        bad.panels[0].rect = rect(1, 1, 99, 12);
        assert!(set_layout_in(&mut layouts, "p-bad", bad).is_err());
        save_store_at(&path, &layouts).unwrap();

        let reloaded = load_store_at(&path);
        assert_eq!(reloaded.len(), 1);
        assert_eq!(get_layout_in(&reloaded, "p-ok"), Some(focus_layout()));
        assert_eq!(get_layout_in(&reloaded, "p-bad"), None);
    }

    #[test]
    fn overwrite_same_key_keeps_latest() {
        let mut layouts = BTreeMap::new();
        set_layout_in(&mut layouts, "p-x", focus_layout()).unwrap();
        let mut build = focus_layout();
        build
            .panels
            .push(panel("terminal", "terminal", rect(1, 9, 12, 4)));
        set_layout_in(&mut layouts, "p-x", build.clone()).unwrap();
        assert_eq!(get_layout_in(&layouts, "p-x"), Some(build));
    }

    #[test]
    fn storage_path_lives_in_shannon_desktop() {
        let path = storage_path().unwrap();
        let rendered = path.to_string_lossy().to_string();
        assert!(
            rendered.contains(".shannon/desktop/workspace-layouts.json"),
            "{rendered}"
        );
    }
}
