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
    /// Theme name (e.g. "default_dark", "default_light", "dracula")
    pub theme: Option<String>,
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
        if let Err(e) = std::fs::create_dir_all(parent) {
            tracing::debug!("Failed to create preferences dir: {e}");
        }
    }
    if let Ok(json) = serde_json::to_string_pretty(prefs) {
        if let Err(e) = std::fs::write(&path, json) {
            tracing::debug!("Failed to save preferences: {e}");
        }
    }
}
