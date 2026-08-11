//! `provider/model` unique model identifier (ADR-0005 Phase 0).
//!
//! A [`ModelRef`] is the canonical cross-cutting spelling for a model: a
//! provider slug + a model id joined by `/` (e.g.
//! `anthropic/claude-sonnet-4-20250514`, `ollama/llama3`). It is the
//! user-facing identifier for `/model`, `--model`, and the desktop shell's
//! provider switcher, and the wire spelling both front-ends speak so a model
//! is never ambiguous about *which provider* serves it.
//!
//! A *bare* model id (no `/`) is still accepted for backward compatibility;
//! the call site resolves the provider from context (the currently active
//! provider) via [`ModelRef::from_input`].
//!
//! This type is intentionally **not** part of [`crate::provider_config`]'s
//! `ProviderModelConfig` schema, so it does not participate in the `build.rs`
//! schema redeclaration (ADR-0004). It maps freely to/from
//! [`ActiveTarget`] when a scope is known.

use crate::provider_config::{ActiveTarget, Scope};
use serde::{Deserialize, Serialize};

/// `provider/model` identifier.
///
/// `provider` is normalized to lowercase (provider slugs are case-insensitive);
/// `model` is kept verbatim because some providers distinguish model ids by
/// case or version suffix.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModelRef {
    /// Provider slug (e.g. `anthropic`, `openai`, `ollama`, `zhipu`).
    pub provider: String,
    /// Model id as sent to the API (e.g. `claude-sonnet-4-20250514`).
    pub model: String,
}

impl ModelRef {
    /// Separator between provider and model in the qualified spelling.
    pub const SEPARATOR: char = '/';

    /// Construct from raw parts. `provider` is lowercased.
    pub fn new(provider: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            provider: provider.into().trim().to_lowercase(),
            model: model.into().trim().to_string(),
        }
    }

    /// True when `s` is the qualified `provider/model` form (contains `/`).
    pub fn is_qualified(s: &str) -> bool {
        s.contains(Self::SEPARATOR)
    }

    /// Parse a qualified `provider/model` string.
    ///
    /// Returns `None` when the string has no separator (bare id), an empty
    /// provider, an empty model, or more than one `/`. For lenient parsing of
    /// bare ids with a context provider, use [`Self::from_input`].
    pub fn parse(s: &str) -> Option<Self> {
        let (provider, model) = s.split_once(Self::SEPARATOR)?;
        let provider = provider.trim();
        let model = model.trim();
        if provider.is_empty() || model.is_empty() || model.contains(Self::SEPARATOR) {
            return None;
        }
        Some(Self::new(provider, model))
    }

    /// Parse a user-supplied model string that may be either a qualified
    /// `provider/model` ref or a bare model id.
    ///
    /// - Qualified (`"anthropic/claude-sonnet-4-20250514"`) → parsed as-is.
    /// - Bare (`"sonnet"`, `"llama3"`) → attached to `fallback_provider`
    ///   (the currently active provider), so legacy `--model sonnet` keeps
    ///   working. Returns `None` only when the input is empty/garbage or a
    ///   bare id is given with no fallback provider.
    pub fn from_input(s: &str, fallback_provider: Option<&str>) -> Option<Self> {
        if let Some(parsed) = Self::parse(s) {
            return Some(parsed);
        }
        let model = s.trim();
        if model.is_empty() || model.contains(Self::SEPARATOR) {
            return None;
        }
        let provider = fallback_provider?.trim();
        if provider.is_empty() {
            return None;
        }
        Some(Self::new(provider, model))
    }

    /// Render as the qualified `provider/model` string.
    pub fn to_qualified(&self) -> String {
        format!("{}{}{}", self.provider, Self::SEPARATOR, self.model)
    }

    /// Map to an [`ActiveTarget`] (the v2 config's active selection record).
    pub fn to_active_target(&self, scope: Scope) -> ActiveTarget {
        ActiveTarget {
            provider_id: self.provider.clone(),
            model_id: self.model.clone(),
            scope,
        }
    }

    /// Build from an [`ActiveTarget`]'s `provider_id` + `model_id`.
    pub fn from_active_target(at: &ActiveTarget) -> Self {
        Self::new(&at.provider_id, &at.model_id)
    }
}

impl std::fmt::Display for ModelRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{}{}", self.provider, Self::SEPARATOR, self.model)
    }
}

impl std::str::FromStr for ModelRef {
    type Err = ModelRefParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s).ok_or_else(|| ModelRefParseError(s.to_string()))
    }
}

/// Error returned by [`ModelRef`]'s [`FromStr`](std::str::FromStr) impl.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("not a valid `provider/model` identifier: {0:?} (expected `provider/model`)")]
pub struct ModelRefParseError(pub String);

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn parse_qualified_basic() {
        let m = ModelRef::parse("anthropic/claude-sonnet-4-20250514").unwrap();
        assert_eq!(m.provider, "anthropic");
        assert_eq!(m.model, "claude-sonnet-4-20250514");
    }

    #[test]
    fn parse_lowercases_provider_keeps_model_verbatim() {
        let m = ModelRef::parse("Anthropic/CLAUDE-X").unwrap();
        assert_eq!(m.provider, "anthropic");
        assert_eq!(m.model, "CLAUDE-X");
    }

    #[test]
    fn parse_trims_whitespace() {
        let m = ModelRef::parse("  ollama / llama3  ").unwrap();
        assert_eq!(m.provider, "ollama");
        assert_eq!(m.model, "llama3");
    }

    #[test]
    fn parse_rejects_bare_id() {
        assert!(ModelRef::parse("claude-sonnet-4-20250514").is_none());
    }

    #[test]
    fn parse_rejects_empty_provider_or_model() {
        assert!(ModelRef::parse("/model").is_none());
        assert!(ModelRef::parse("anthropic/").is_none());
        assert!(ModelRef::parse("/").is_none());
    }

    #[test]
    fn parse_rejects_extra_separators() {
        // model part containing `/` is not a valid ref
        assert!(ModelRef::parse("anthropic/claude/opus").is_none());
    }

    #[test]
    fn parse_rejects_empty() {
        assert!(ModelRef::parse("").is_none());
    }

    #[test]
    fn from_input_qualified_wins() {
        let m = ModelRef::from_input("openai/gpt-4o", Some("anthropic")).unwrap();
        assert_eq!(m.provider, "openai");
        assert_eq!(m.model, "gpt-4o");
    }

    #[test]
    fn from_input_bare_uses_fallback_provider() {
        let m = ModelRef::from_input("sonnet", Some("anthropic")).unwrap();
        assert_eq!(m.provider, "anthropic");
        assert_eq!(m.model, "sonnet");
    }

    #[test]
    fn from_input_bare_without_fallback_is_none() {
        assert!(ModelRef::from_input("sonnet", None).is_none());
    }

    #[test]
    fn from_input_empty_is_none() {
        assert!(ModelRef::from_input("   ", Some("anthropic")).is_none());
        assert!(ModelRef::from_input("", None).is_none());
    }

    #[test]
    fn from_input_bare_falls_through_parse_failure_not_split_once() {
        // A bare id with no separator should not be misread; ensure model text
        // containing no `/` but failing qualified parse goes to fallback.
        let m = ModelRef::from_input("llama3", Some("ollama")).unwrap();
        assert_eq!(m.to_qualified(), "ollama/llama3");
    }

    #[test]
    fn display_and_to_qualified_match() {
        let m = ModelRef::new("anthropic", "claude-opus-4-20250115");
        assert_eq!(m.to_qualified(), "anthropic/claude-opus-4-20250115");
        assert_eq!(format!("{m}"), "anthropic/claude-opus-4-20250115");
    }

    #[test]
    fn is_qualified_detects_separator() {
        assert!(ModelRef::is_qualified("anthropic/sonnet"));
        assert!(!ModelRef::is_qualified("sonnet"));
    }

    #[test]
    fn from_str_roundtrips_display() {
        let s = "ollama/llama3";
        let m: ModelRef = s.parse().unwrap();
        assert_eq!(m.to_string(), s);
    }

    #[test]
    fn from_str_bare_errors() {
        let err = "bare-id".parse::<ModelRef>().unwrap_err();
        assert_eq!(err.0, "bare-id");
    }

    #[test]
    fn to_active_target_round_trips() {
        let m = ModelRef::new("anthropic", "claude-sonnet-4-20250514");
        let at = m.to_active_target(Scope::Global);
        assert_eq!(at.provider_id, "anthropic");
        assert_eq!(at.model_id, "claude-sonnet-4-20250514");
        assert_eq!(at.scope, Scope::Global);
        // And back:
        let m2 = ModelRef::from_active_target(&at);
        assert_eq!(m, m2);
    }

    #[test]
    fn serde_roundtrips_as_object() {
        let m = ModelRef::new("zhipu", "glm-4.6");
        let json = serde_json::to_string(&m).unwrap();
        let back: ModelRef = serde_json::from_str(&json).unwrap();
        assert_eq!(m, back);
        // Fields serialize by name.
        assert!(json.contains("\"provider\""));
        assert!(json.contains("\"model\""));
    }

    #[test]
    fn equality_and_hash() {
        use std::collections::HashSet;
        let a = ModelRef::new("Anthropic", "sonnet"); // provider lowercased
        let b = ModelRef::new("anthropic", "sonnet");
        assert_eq!(a, b);
        let mut set = HashSet::new();
        set.insert(a);
        assert!(set.contains(&b));
    }
}
