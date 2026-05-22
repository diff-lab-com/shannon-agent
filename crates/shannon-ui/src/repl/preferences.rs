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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_default() {
        let prefs = Preferences::default();
        assert!(prefs.model.is_none());
        assert!(prefs.provider.is_none());
        assert!(prefs.theme.is_none());
    }

    #[test]
    fn preferences_serde_roundtrip() {
        let prefs = Preferences {
            model: Some("claude-3-opus".to_string()),
            provider: Some(LlmProvider::Anthropic),
            theme: Some("dracula".to_string()),
        };
        let json = serde_json::to_string(&prefs).unwrap();
        let back: Preferences = serde_json::from_str(&json).unwrap();
        assert_eq!(back.model, Some("claude-3-opus".to_string()));
        assert_eq!(back.provider, Some(LlmProvider::Anthropic));
        assert_eq!(back.theme, Some("dracula".to_string()));
    }

    #[test]
    fn preferences_partial_serde() {
        let json = r#"{"model":"gpt-4"}"#;
        let prefs: Preferences = serde_json::from_str(json).unwrap();
        assert_eq!(prefs.model, Some("gpt-4".to_string()));
        assert!(prefs.provider.is_none());
        assert!(prefs.theme.is_none());
    }

    #[test]
    fn preferences_empty_json() {
        let prefs: Preferences = serde_json::from_str("{}").unwrap();
        assert!(prefs.model.is_none());
        assert!(prefs.provider.is_none());
    }

    #[test]
    fn preferences_debug_format() {
        let prefs = Preferences {
            model: Some("test".to_string()),
            provider: None,
            theme: None,
        };
        let debug = format!("{prefs:?}");
        assert!(debug.contains("test"));
    }

    #[test]
    fn preferences_clone() {
        let prefs = Preferences {
            model: Some("claude-3".to_string()),
            provider: Some(LlmProvider::OpenAI),
            theme: None,
        };
        let cloned = prefs.clone();
        assert_eq!(cloned.model, prefs.model);
        assert_eq!(cloned.provider, prefs.provider);
    }
}
