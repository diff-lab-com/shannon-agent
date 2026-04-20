//! Persisted user preferences (model, provider).
//!
//! Stored at `~/.shannon/preferences.json`. Loaded on startup, saved whenever
//! the user changes model or provider via `/model`, model picker, or input dialog.

use std::path::PathBuf;
use shannon_core::api::LlmProvider;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct Preferences {
    pub model: Option<String>,
    pub provider: Option<LlmProvider>,
}

fn preferences_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".shannon").join("preferences.json"))
}

/// Load persisted preferences from disk. Returns `Default` if file is missing
/// or cannot be parsed (first run, corruption, etc.).
pub fn load_preferences() -> Preferences {
    let Some(path) = preferences_path() else {
        return Preferences::default();
    };
    match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => Preferences::default(),
    }
}

/// Save preferences to disk. Creates `~/.shannon/` if it doesn't exist.
pub fn save_preferences(prefs: &Preferences) {
    let Some(path) = preferences_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(prefs) {
        let _ = std::fs::write(&path, json);
    }
}
