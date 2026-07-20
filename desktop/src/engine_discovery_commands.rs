//! Tauri commands for engine discovery.

use crate::commands::AppState;
use crate::engine_discovery::EngineMode;
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineModeInfo {
    pub mode: Option<EngineMode>,
}

/// Return the resolved engine mode after startup. `None` until the
/// probe has completed; `Some(Hosted)` or `Some(External)` after.
#[tauri::command]
pub async fn engine_discovery_get_mode(
    state: tauri::State<'_, AppState>,
) -> Result<EngineModeInfo, String> {
    let mode = *state.engine_mode.read().expect("engine_mode lock poisoned");
    Ok(EngineModeInfo { mode })
}
