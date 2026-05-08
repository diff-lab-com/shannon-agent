//! Internationalization (i18n) support for Shannon Code.
//!
//! Provides a `t!` macro wrapper for translations, plus helper functions
//! for setting and querying the current locale.

/// Translate a key using the current locale.
///
/// This is a convenience wrapper around `rust_i18n::t!` so that downstream
/// crates only need to depend on `shannon_core` for translations.
///
/// # Example
///
/// ```ignore
/// use shannon_core::i18n::t;
///
/// let msg = t!("repl.chat_cleared");
/// let msg = t!("repl.unknown_command", name = "foo");
/// ```
pub use rust_i18n::t;

/// Set the active locale for translations.
///
/// Valid values include `"en"`, `"zh"`, `"hi"`, `"es"`, `"fr"`, `"ar"`, `"bn"`,
/// `"pt"`, `"ru"`, `"ja"`. Falls back to English if the requested locale is unavailable.
pub fn set_locale(lang: &str) {
    rust_i18n::set_locale(lang);
}

/// Get the currently active locale identifier.
///
/// Returns a string like `"en"` or `"zh"`.
pub fn current_locale() -> String {
    rust_i18n::locale().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All tests use explicit `locale =` to avoid interference from parallel
    /// test threads sharing the global locale state.

    #[test]
    fn test_translate_english() {
        assert_eq!(t!("repl.chat_cleared", locale = "en"), "Chat cleared.");
        assert_eq!(t!("status.ready", locale = "en"), "Ready");
        assert_eq!(t!("status.processing", locale = "en"), "Processing...");
        assert_eq!(t!("status.loading", locale = "en"), "Loading...");
    }

    #[test]
    fn test_translate_chinese() {
        assert_eq!(t!("repl.chat_cleared", locale = "zh"), "聊天已清空。");
        assert_eq!(t!("status.ready", locale = "zh"), "就绪");
        assert_eq!(t!("status.processing", locale = "zh"), "处理中...");
        assert_eq!(t!("status.loading", locale = "zh"), "加载中...");
    }

    #[test]
    fn test_translate_with_variable_english() {
        assert_eq!(t!("commands.model.set", locale = "en", name = "gpt-4o"), "Model set to: gpt-4o");
    }

    #[test]
    fn test_translate_with_variable_chinese() {
        assert_eq!(t!("commands.model.set", locale = "zh", name = "gpt-4o"), "模型已设置为: gpt-4o");
    }

    #[test]
    fn test_set_and_get_locale() {
        set_locale("en");
        assert_eq!(current_locale(), "en");
        set_locale("zh");
        assert_eq!(current_locale(), "zh");
        set_locale("en");
    }

    #[test]
    fn test_fallback_to_english() {
        // All keys should resolve in English without being empty
        assert!(!t!("repl.chat_cleared", locale = "en").is_empty());
        assert!(!t!("status.ready", locale = "en").is_empty());
        assert!(!t!("cli.about", locale = "en").is_empty());
    }

    #[test]
    fn test_all_locales_resolve() {
        for lang in ["en", "zh", "hi", "es", "fr", "ar", "bn", "pt", "ru", "ja"] {
            let msg = t!("repl.chat_cleared", locale = lang);
            assert!(!msg.is_empty(), "locale '{lang}' returned empty for repl.chat_cleared");
        }
    }
}
