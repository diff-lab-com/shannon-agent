//! # RedactionPolicy (§4.14) — the full write-path secret policy
//!
//! Replaces the §4.2 minimal mask with a composable policy applied at
//! **write time** (plan invariant: the log on disk is clean — no post-hoc
//! scrubbing pass exists or is needed). One [`RedactionPolicy`] instance
//! bundles every masking source and applies them in a widening order:
//!
//! 1. Built-in token shapes (provider keys, GitHub/Slack/GitLab tokens)
//!    — compiled once, cannot be disabled by configuration (fail closed).
//! 2. User extra prefixes from `redaction.toml` — literal prefixes treated
//!    like the built-in ones.
//! 3. User regexes from `redaction.toml` — any match is masked.
//! 4. Explicit values from `redaction.toml` plus the snapshot of process
//!    env vars whose name looks like a secret (`KEY` / `SECRET` / `TOKEN`
//!    / `PASSWORD`) — masked verbatim wherever they appear.
//!
//! Configuration lives at `~/.shannon/redaction.toml` (`SHANNON_HOME`
//! relocates it); an explicit path override is available via
//! `SHANNON_REDACTION_TOML`. The file never changes what is already
//! masked — it can only add rules. Broken entries (invalid regex,
//! too-short value) are skipped with a warning; loading never fails
//! logging.
//!
//! Every [`SessionTee`](super::SessionTee) captures one immutable policy
//! snapshot when it opens and uses it for the whole query, so a running
//! turn cannot observe a half-edited file.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use tracing::warn;

/// Replacement token for everything the policy masks.
pub const REDACTED: &str = "[REDACTED]";

/// A candidate secret value must be at least this many chars, so trivial
/// env values ("1", "yes") do not shred ordinary text.
pub const MIN_SECRET_VALUE_LEN: usize = 8;

// ============================================================================
// Built-in layer (the §4.2 minimal set, verbatim semantics)
// ============================================================================

/// Token shapes masked wherever they appear: provider keys, GitHub PATs,
/// Slack tokens, GitLab PATs. Fixed — user config extends it, never narrows.
pub static BUILTIN_PREFIX_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(concat!(
        r"(sk-[A-Za-z0-9_-]{8,}",
        r"|ghp_[A-Za-z0-9]{8,}",
        r"|github_pat_[A-Za-z0-9_]{8,}",
        r"|xox[abp]-[A-Za-z0-9-]{8,}",
        r"|glpat-[A-Za-z0-9_-]{8,})",
    ))
    .expect("builtin redaction regex compiles")
});

/// Env var name that suggests its value is a secret.
fn env_name_looks_secret(name: &str) -> bool {
    let upper = name.to_uppercase();
    upper.contains("KEY")
        || upper.contains("SECRET")
        || upper.contains("TOKEN")
        || upper.contains("PASSWORD")
}

// ============================================================================
// Configuration file schema (~/.shannon/redaction.toml)
// ============================================================================

/// The on-disk shape of `redaction.toml`. All sections optional; unknown
/// fields are rejected by serde so typos surface instead of silently not
/// matching.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RedactionConfig {
    /// Extra literal token prefixes (extended form of rule 2).
    #[serde(default)]
    prefixes: PrefixesSection,
    /// Extra regex rules (rule 3).
    #[serde(default)]
    patterns: Vec<PatternEntry>,
    /// Extra explicit secret values (rule 4).
    #[serde(default)]
    values: ValuesSection,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrefixesSection {
    /// Literal prefixes whose continuations are masked.
    #[serde(default)]
    extra: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct PatternEntry {
    /// Regex matched anywhere in the logged string.
    regex: String,
    /// Optional custom replacement; defaults to `[REDACTED]`.
    #[serde(default)]
    replacement: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ValuesSection {
    /// Exact values to mask verbatim.
    #[serde(default)]
    secrets: Vec<String>,
}

impl RedactionConfig {
    fn parse(text: &str, origin: &Path) -> Option<Self> {
        match toml::from_str(text) {
            Ok(cfg) => Some(cfg),
            Err(e) => {
                warn!(path = %origin.display(), error = %e, "redaction.toml ignored: parse failed");
                None
            }
        }
    }
}

// ============================================================================
// Policy
// ============================================================================

/// One immutable bundle of masking rules. See the module docs for the
/// application order; each later rule can only widen coverage.
#[derive(Debug, Clone)]
pub struct RedactionPolicy {
    extra_prefix_regex: Option<Regex>,
    user_rules: Vec<(Regex, String)>,
    exact_values: Vec<String>,
}

impl Default for RedactionPolicy {
    fn default() -> Self {
        Self::from_config(&RedactionConfig::default())
    }
}

impl RedactionPolicy {
    /// Build from a parsed config (no env snapshot — see [`Self::capture`]).
    fn from_config(config: &RedactionConfig) -> Self {
        // Rule 2 — extra literal prefixes joined into one alternation;
        // a continuation of >= MIN_SECRET_VALUE_LEN chars must follow.
        let mut joined = String::new();
        for prefix in &config.prefixes.extra {
            if prefix.is_empty() {
                continue;
            }
            if !joined.is_empty() {
                joined.push('|');
            }
            joined.push_str(&format!(
                "({}[A-Za-z0-9._~+/=-]{{{MIN},}})",
                regex::escape(prefix),
                MIN = MIN_SECRET_VALUE_LEN
            ));
        }
        let extra_prefix_regex = (!joined.is_empty())
            .then(|| Regex::new(&joined).expect("escaped-literal prefix regex compiles"));

        // Rule 3 — user regexes with optional replacements.
        let mut user_rules = Vec::new();
        for entry in &config.patterns {
            let replacement = entry
                .replacement
                .clone()
                .unwrap_or_else(|| REDACTED.to_string());
            match Regex::new(&entry.regex) {
                Ok(re) => user_rules.push((re, replacement)),
                Err(e) => {
                    warn!(pattern = %entry.regex, error = %e, "redaction pattern ignored: invalid regex");
                }
            }
        }

        // Rule 4a — configured values (length-guarded).
        let mut exact_values: Vec<String> = config
            .values
            .secrets
            .iter()
            .filter(|v| v.chars().count() >= MIN_SECRET_VALUE_LEN && v.as_str() != REDACTED)
            .filter(|v| !BUILTIN_PREFIX_REGEX.is_match(v))
            .cloned()
            .collect();

        // Rule 4b — env snapshot. Names look-secret AND value long enough.
        // Later additions replace earlier duplicates cheaply via dedup below.
        for (name, value) in std::env::vars() {
            if env_name_looks_secret(&name)
                && value.len() >= MIN_SECRET_VALUE_LEN
                && value != REDACTED
                && !BUILTIN_PREFIX_REGEX.is_match(&value)
            {
                exact_values.push(value);
            }
        }
        exact_values.sort_unstable();
        exact_values.dedup();

        Self {
            extra_prefix_regex,
            user_rules,
            exact_values,
        }
    }

    /// Load the config file at `path`, folding in the live env snapshot.
    ///
    /// Infallible: a missing file yields the default (built-ins only), a
    /// broken file degrades to built-ins with a warning.
    pub fn load(path: &Path) -> Self {
        let config = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| RedactionConfig::parse(&text, path))
            .unwrap_or_default();
        Self::from_config(&config)
    }

    /// Snapshot of the default policy + env secrets, with no file read.
    pub fn capture_env_only() -> Self {
        Self::from_config(&RedactionConfig::default())
    }

    /// Resolve the effective policy location:
    /// `$SHANNON_REDACTION_TOML` > `<shannon-home>/redaction.toml`
    /// (used only when present) > env-only defaults.
    pub fn resolve() -> Arc<Self> {
        let path = config_path_override().or_else(|| {
            crate::session_log::default_shannon_home()
                .ok()
                .map(|home| home.join("redaction.toml"))
        });
        match path {
            Some(p) if p.is_file() => Arc::new(Self::load(&p)),
            Some(p) if config_path_override().is_some() => {
                // Explicit override pointing at a missing file: say so.
                warn!(path = %p.display(), "SHANNON_REDACTION_TOML does not exist; using defaults");
                Arc::new(Self::capture_env_only())
            }
            _ => Arc::new(Self::capture_env_only()),
        }
    }

    /// Number of exact-value rules currently bundled (tests + diagnostics).
    pub fn exact_value_count(&self) -> usize {
        self.exact_values.len()
    }

    /// Mask every secret form inside `text`.
    pub fn redact_str(&self, text: &str) -> String {
        let mut out = BUILTIN_PREFIX_REGEX
            .replace_all(text, REDACTED)
            .into_owned();
        if let Some(extra) = &self.extra_prefix_regex {
            if extra.is_match(&out) {
                out = extra.replace_all(&out, REDACTED).into_owned();
            }
        }
        for (regex, replacement) in &self.user_rules {
            if regex.is_match(&out) {
                out = regex.replace_all(&out, replacement.as_str()).into_owned();
            }
        }
        for value in &self.exact_values {
            if out.contains(value.as_str()) {
                out = out.replace(value.as_str(), REDACTED);
            }
        }
        out
    }

    /// Recursively mask every string inside a JSON value.
    pub fn redact_value(&self, value: &Value) -> Value {
        match value {
            Value::String(s) => Value::String(self.redact_str(s)),
            Value::Array(items) => {
                Value::Array(items.iter().map(|v| self.redact_value(v)).collect())
            }
            Value::Object(map) => Value::Object(
                map.iter()
                    .map(|(k, v)| (k.clone(), self.redact_value(v)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }
}

// ============================================================================
// Process-wide helpers (compat surface used across the session_log module)
// ============================================================================

static GLOBAL_POLICY: Lazy<Arc<RedactionPolicy>> = Lazy::new(RedactionPolicy::resolve);

/// The process-wide policy snapshot (first use wins, matching the old
/// Lazy-static minimal set's behavior).
pub fn global_policy() -> Arc<RedactionPolicy> {
    Arc::clone(&GLOBAL_POLICY)
}

/// Path override resolution shared by tests and docs.
fn config_path_override() -> Option<PathBuf> {
    std::env::var("SHANNON_REDACTION_TOML")
        .ok()
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// Mask secrets in `text` using the process-wide policy snapshot.
pub fn redact_string(text: &str) -> String {
    global_policy().redact_str(text)
}

/// Core pairing: builtin prefixes plus caller-provided exact values — kept
/// as the stable unit-test seam for "these N strings get masked here".
pub fn redact_with_secrets(text: &str, secrets: &[String]) -> String {
    let mut out = BUILTIN_PREFIX_REGEX
        .replace_all(text, REDACTED)
        .into_owned();
    for secret in secrets {
        if out.contains(secret.as_str()) {
            out = out.replace(secret.as_str(), REDACTED);
        }
    }
    out
}

/// Recursively redact with the process-wide policy snapshot.
pub fn redact_value(value: &Value) -> Value {
    global_policy().redact_value(value)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use serde_json::json;

    fn empty_toml() -> PathBuf {
        // A path that certainly doesn't exist: load() must fall back to
        // defaults without erroring.
        PathBuf::from("/nonexistent/redaction.toml")
    }

    #[test]
    fn builtin_prefixes_still_masked() {
        let policy = RedactionPolicy::load(&empty_toml());
        let text = "key sk-ant-abc123456789 and pat ghp_abcdefgh12345678 slack xoxb-12345678-abcdefg plain text";
        let redacted = policy.redact_str(text);
        assert!(!redacted.contains("sk-ant-abc123456789"));
        assert!(!redacted.contains("ghp_abcdefgh12345678"));
        assert!(!redacted.contains("xoxb-12345678-abcdefg"));
        assert!(redacted.contains("plain text"));
        assert_eq!(redacted.matches(REDACTED).count(), 3);

        // GitHub fine-grained PAT and GitLab PAT shapes too.
        let more = "a github_pat_ABCDEFGHI123456 b glpat-tuvwx56789yz c";
        let redacted = policy.redact_str(more);
        assert_eq!(redacted.matches(REDACTED).count(), 2);
    }

    #[test]
    fn load_parses_extra_prefix_patterns_and_values() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("redaction.toml");
        std::fs::write(
            &path,
            concat!(
                "[prefixes]\n",
                "extra = [\"xapp-1-\"]\n",
                "\n",
                "[[patterns]]\n",
                "regex = 'internal-ticket-[0-9]{4}'\n",
                "\n",
                "[[patterns]]\n",
                "regex = \"(bad)[\"\n",
                "# invalid regex above is skipped with a warning\n",
                "\n",
                "[values]\n",
                "secrets = [\"hardcoded-shared-secret\"]\n"
            ),
        )
        .unwrap();

        let policy = RedactionPolicy::load(&path);
        let text =
            "token xapp-1-a1b2c3d4e5f6 ref internal-ticket-2026 val hardcoded-shared-secret end";
        let redacted = policy.redact_str(text);
        assert!(!redacted.contains("xapp-1-a1b2c3d4e5f6"), "extra prefix");
        assert!(!redacted.contains("internal-ticket-2026"), "user regex");
        assert!(!redacted.contains("hardcoded-shared-secret"), "value");
        assert_eq!(redacted.matches(REDACTED).count(), 3);

        // Short configured values are dropped by the length guard instead of
        // shredding text that happens to contain them.
        let short = dir.path().join("short.toml");
        std::fs::write(&short, "[values]\nsecrets = [\"abc\"]\n").unwrap();
        let policy = RedactionPolicy::load(&short);
        assert_eq!(policy.redact_str("abcd abc abcd"), "abcd abc abcd");
    }

    #[test]
    fn load_never_fails_on_broken_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("broken.toml");
        std::fs::write(&path, "not valid toml [[[").unwrap();
        // Falls back to builtins-only behavior.
        let policy = RedactionPolicy::load(&path);
        assert_eq!(policy.redact_str("k sk-abcdefghijklmnop"), "k [REDACTED]");
        // Unknown fields reject the whole file (typo protection) rather than
        // silently half-applying it.
        let typo = dir.path().join("typo.toml");
        std::fs::write(&typo, "[prefxes]\nextra = [\"x\"]\n").unwrap();
        let policy = RedactionPolicy::load(&typo);
        assert_eq!(policy.redact_str("plain"), "plain");
    }

    #[test]
    fn env_secrets_are_snapshotted_into_exact_values() {
        // Direct builder path (no process env mutation): simulate the env
        // contribution by checking a policy built from a config containing a
        // KEY-named var through the documented filter.
        let name = "TEST_FAKE_API_KEY";
        let saved = std::env::var(name).ok();
        unsafe {
            std::env::set_var(name, "env-injected-secret-value");
        }
        let policy = RedactionPolicy::capture_env_only();
        let count = policy.exact_value_count();
        if let Some(old) = saved {
            unsafe {
                std::env::set_var(name, old);
            }
        } else {
            unsafe {
                std::env::remove_var(name);
            }
        }
        assert!(
            count > 0,
            "at least the injected key-shaped env value lands in the policy"
        );
    }

    #[test]
    fn recursive_json_redaction() {
        let policy = RedactionPolicy::default();
        let value = json!({"a": ["sk-ant-abcdefghijk"], "b": {"c": 3}});
        assert_eq!(
            policy.redact_value(&value),
            json!({"a": ["[REDACTED]"], "b": {"c": 3}})
        );
    }

    #[test]
    fn custom_replacement_applies() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("repl.toml");
        std::fs::write(
            &path,
            "[[patterns]]\nregex = 'hunter2'\nreplacement = '<removed>'\n",
        )
        .unwrap();
        let policy = RedactionPolicy::load(&path);
        assert_eq!(policy.redact_str("pw hunter2 ok"), "pw <removed> ok");
    }
}
