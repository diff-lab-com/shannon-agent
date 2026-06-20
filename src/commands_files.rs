//! File-related commands — text save, diff, apply, tree, working-dir info.
//!
//! Extracted from `commands.rs` as part of S2 P1.1 (commands.rs split).
//! More file commands will move here in future extractions.


/// Write text content to a file, creating parent directories as needed.
#[tauri::command]
pub async fn save_text_file(path: String, content: String) -> Result<(), String> {
    let target = std::path::Path::new(&path);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create {}: {e}", parent.display()))?;
    }
    std::fs::write(target, content)
        .map_err(|e| format!("Failed to write {}: {e}", target.display()))
}

