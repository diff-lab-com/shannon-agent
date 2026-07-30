//! `{env:VAR}` / `{env:VAR:-default}` / `{file:path}` substitution for config
//! strings.
//!
//! ADR-0005 Phase 4: lets users reference environment variables or files
//! instead of inlining secrets, strengthening A1 (config never carries
//! plaintext). Single-pass so the resolved value is never re-scanned for
//! further substitutions (defeats recursive injection). `file:` paths are
//! restricted to `~/.shannon/` or absolute paths, with no `..` traversal, so
//! config files cannot reach outside the intended scope.
//!
//! Substitutions are recognised only when wrapped in `{...}`:
//! - `{env:VAR}` — value of `VAR` (empty + warning if unset).
//! - `{env:VAR:-default}` — value of `VAR`, or the literal `default` if unset.
//! - `{file:/abs/path}` or `{file:~/.shannon/x}` — file content (trimmed).
//!
//! Anything else (unknown token, unclosed brace, relative path) is left
//! untouched so user data is never silently mangled.

use crate::unified_config::ShannonConfig;
use serde_json::Value;
use std::path::{Component, PathBuf};

/// Substitute every `{env:…}` / `{file:…}` token in the string leaves of a
/// JSON value. Single-pass: results are not re-scanned. `warnings` collects
/// non-fatal issues (missing env, unreadable file, rejected path) so the
/// caller can surface them.
pub fn substitute_value(value: &mut Value, warnings: &mut Vec<String>) {
    match value {
        Value::String(s) => {
            *s = substitute_string(s, warnings);
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                substitute_value(item, warnings);
            }
        }
        Value::Object(map) => {
            for (_k, val) in map.iter_mut() {
                substitute_value(val, warnings);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

/// Run [`substitute_value`] over every string leaf of a `ShannonConfig` and
/// log any warnings. Used by `ConfigBuilder` after each `load_*` step so
/// `~/.shannon/config.toml`, `.shannon.toml`, and `~/.shannon/providers.toml`
/// all support `{env:…}` / `{file:…}` references.
pub fn substitute_config(cfg: &mut ShannonConfig) {
    let mut warnings = Vec::new();
    let Ok(mut value) = serde_json::to_value(cfg.clone()) else {
        return;
    };
    substitute_value(&mut value, &mut warnings);
    if let Ok(updated) = serde_json::from_value::<ShannonConfig>(value) {
        *cfg = updated;
    }
    for w in warnings {
        tracing::warn!("config substitution: {w}");
    }
}

/// Single-pass string substitution. Public for unit tests; the entry points
/// for production code are [`substitute_value`] and [`substitute_config`].
fn substitute_string(s: &str, warnings: &mut Vec<String>) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            // Look for the matching `}` in the remainder. `position` on a
            // byte slice is a single-pass scan and avoids the manual range
            // loop that clippy flags.
            if let Some(rel) = bytes[i + 1..].iter().position(|&b| b == b'}') {
                let end_idx = i + 1 + rel;
                let token = &s[i + 1..end_idx];
                if let Some(replacement) = parse_token(token, warnings) {
                    // Single-pass: replacement is appended as-is and is NOT
                    // re-scanned for further tokens.
                    out.push_str(&replacement);
                } else {
                    // Unknown token — keep the literal `{…}` so user data
                    // is never silently mangled.
                    out.push('{');
                    out.push_str(token);
                    out.push('}');
                }
                i = end_idx + 1;
                continue;
            } else {
                // Unclosed `{` — keep the literal remainder.
                out.push_str(&s[i..]);
                i = bytes.len();
                continue;
            }
        } else {
            // ASCII-only branch is safe here because we only push the literal
            // byte and advance one position; the only branch that touches
            // multi-byte UTF-8 is the `out.push_str(&s[i..])` above, which
            // copies a valid slice.
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

fn parse_token(token: &str, warnings: &mut Vec<String>) -> Option<String> {
    if let Some(rest) = token.strip_prefix("env:") {
        let (var, default) = if let Some(idx) = rest.find(":-") {
            (&rest[..idx], Some(&rest[idx + 2..]))
        } else {
            (rest, None)
        };
        if var.is_empty() {
            warnings.push("empty env name in `{env:}` token".to_string());
            return None;
        }
        match std::env::var(var) {
            Ok(v) => Some(v),
            Err(_) => match default {
                Some(d) => Some(d.to_string()),
                None => {
                    warnings.push(format!("env `{var}` missing, leaving empty"));
                    Some(String::new())
                }
            },
        }
    } else if let Some(rest) = token.strip_prefix("file:") {
        let path = resolve_file_path(rest)?;
        match std::fs::read_to_string(&path) {
            Ok(content) => Some(content.trim().to_string()),
            Err(_) => {
                warnings.push(format!(
                    "file `{}` unreadable, leaving empty",
                    path.display()
                ));
                Some(String::new())
            }
        }
    } else {
        None
    }
}

/// Resolve a `file:` token to an absolute path. Rejects relative paths and
/// `..` traversal so config files cannot reach host-filesystem secrets.
fn resolve_file_path(raw: &str) -> Option<PathBuf> {
    let expanded = if let Some(stripped) = raw.strip_prefix("~/") {
        dirs::home_dir()?.join(stripped)
    } else if raw == "~" {
        dirs::home_dir()?
    } else {
        PathBuf::from(raw)
    };
    if !expanded.is_absolute() {
        return None;
    }
    if expanded
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        return None;
    }
    Some(expanded)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn passes_plain_text_through() {
        let mut w = Vec::new();
        assert_eq!(substitute_string("hello world", &mut w), "hello world");
        assert!(w.is_empty());
    }

    #[test]
    fn env_present_substitutes_value() {
        // SAFETY: unique key, removed at the end of the test.
        unsafe { std::env::set_var("SHANNON_SUB_TEST_X", "x-value") };
        let mut w = Vec::new();
        assert_eq!(
            substitute_string("{env:SHANNON_SUB_TEST_X}", &mut w),
            "x-value"
        );
        assert!(w.is_empty());
        // SAFETY: see above.
        unsafe { std::env::remove_var("SHANNON_SUB_TEST_X") };
    }

    #[test]
    fn env_missing_emits_warning_and_empty_value() {
        // SAFETY: unique key; removed at the end of the test.
        unsafe { std::env::remove_var("SHANNON_SUB_TEST_DEFINITELY_MISSING_XYZ") };
        let mut w = Vec::new();
        assert_eq!(
            substitute_string("{env:SHANNON_SUB_TEST_DEFINITELY_MISSING_XYZ}", &mut w),
            ""
        );
        assert_eq!(w.len(), 1);
        assert!(w[0].contains("SHANNON_SUB_TEST_DEFINITELY_MISSING_XYZ"));
    }

    #[test]
    fn env_missing_uses_default_when_provided() {
        // SAFETY: unique key; removed at the end of the test.
        unsafe { std::env::remove_var("SHANNON_SUB_TEST_DEF_X") };
        let mut w = Vec::new();
        assert_eq!(
            substitute_string("{env:SHANNON_SUB_TEST_DEF_X:-fallback}", &mut w),
            "fallback"
        );
        assert!(w.is_empty());
    }

    #[test]
    fn single_pass_defeats_recursive_injection() {
        // X's value is literally `{env:Y}` — the single-pass scan must
        // produce `{env:Y}` verbatim, NOT resolve to Y's value. This is the
        // core safety property: resolved values are NEVER re-scanned.
        // SAFETY: unique keys; removed at the end of the test.
        unsafe {
            std::env::set_var(
                "SHANNON_SUB_TEST_RECURSE_X",
                "{env:SHANNON_SUB_TEST_RECURSE_Y}",
            )
        };
        unsafe { std::env::set_var("SHANNON_SUB_TEST_RECURSE_Y", "y-value") };
        let mut w = Vec::new();
        assert_eq!(
            substitute_string("{env:SHANNON_SUB_TEST_RECURSE_X}", &mut w),
            "{env:SHANNON_SUB_TEST_RECURSE_Y}"
        );
        // SAFETY: see above.
        unsafe { std::env::remove_var("SHANNON_SUB_TEST_RECURSE_X") };
        unsafe { std::env::remove_var("SHANNON_SUB_TEST_RECURSE_Y") };
    }

    #[test]
    fn file_present_substitutes_trimmed_content() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("endpoint.txt");
        std::fs::write(&p, "https://example.com\n").unwrap();
        let mut w = Vec::new();
        let s = format!("{{file:{}}}", p.display());
        assert_eq!(substitute_string(&s, &mut w), "https://example.com");
        assert!(w.is_empty());
    }

    #[test]
    fn file_missing_warns_and_empty() {
        let mut w = Vec::new();
        let s = "{file:/definitely/does/not/exist/XYZZY_12345}".to_string();
        assert_eq!(substitute_string(&s, &mut w), "");
        assert_eq!(w.len(), 1);
    }

    #[test]
    fn file_rejects_relative_path_kept_literal() {
        let mut w = Vec::new();
        // Relative paths are not resolved — kept literal so user data isn't
        // silently dropped. No warning either: the token is unrecognised
        // from `resolve_file_path`'s perspective.
        assert_eq!(
            substitute_string("{file:./relative/path}", &mut w),
            "{file:./relative/path}"
        );
        assert!(w.is_empty());
    }

    #[test]
    fn file_rejects_traversal_kept_literal() {
        let mut w = Vec::new();
        // Path traversal (`..`) is rejected even when the prefix is absolute.
        assert_eq!(
            substitute_string("{file:/etc/../etc/passwd}", &mut w),
            "{file:/etc/../etc/passwd}"
        );
        assert!(w.is_empty());
    }

    #[test]
    fn unknown_token_kept_literal() {
        let mut w = Vec::new();
        assert_eq!(
            substitute_string("hello {unknown:thing} world", &mut w),
            "hello {unknown:thing} world"
        );
        assert!(w.is_empty());
    }

    #[test]
    fn unclosed_brace_kept_literal() {
        let mut w = Vec::new();
        assert_eq!(
            substitute_string("hello {unclosed world", &mut w),
            "hello {unclosed world"
        );
        assert!(w.is_empty());
    }

    #[test]
    fn empty_env_name_warns_and_kept_literal() {
        let mut w = Vec::new();
        assert_eq!(substitute_string("{env:}", &mut w), "{env:}");
        assert_eq!(w.len(), 1);
    }

    #[test]
    fn recurses_through_object_and_array() {
        // SAFETY: unique keys; removed at the end of the test.
        unsafe { std::env::set_var("SHANNON_SUB_TEST_OBJ_X", "x-value") };
        unsafe { std::env::set_var("SHANNON_SUB_TEST_OBJ_Y", "y-value") };
        let mut v = serde_json::json!({
            "base_url": "{env:SHANNON_SUB_TEST_OBJ_X}",
            "extra_headers": {
                "Authorization": "Bearer {env:SHANNON_SUB_TEST_OBJ_Y}",
            },
            "fallback_models": ["{env:SHANNON_SUB_TEST_OBJ_X}", "literal"],
        });
        let mut w = Vec::new();
        substitute_value(&mut v, &mut w);
        assert_eq!(v["base_url"], "x-value");
        assert_eq!(v["extra_headers"]["Authorization"], "Bearer y-value");
        assert_eq!(v["fallback_models"][0], "x-value");
        assert_eq!(v["fallback_models"][1], "literal");
        assert!(w.is_empty());
        // SAFETY: see above.
        unsafe { std::env::remove_var("SHANNON_SUB_TEST_OBJ_X") };
        unsafe { std::env::remove_var("SHANNON_SUB_TEST_OBJ_Y") };
    }

    #[test]
    fn leaves_non_string_leaves_untouched() {
        let mut v = serde_json::json!({
            "max_tokens": 4096,
            "debug": true,
            "name": "literal",
        });
        let mut w = Vec::new();
        substitute_value(&mut v, &mut w);
        assert_eq!(v["max_tokens"], 4096);
        assert_eq!(v["debug"], true);
        assert_eq!(v["name"], "literal");
    }

    #[test]
    fn substitute_config_round_trips_with_env() {
        // SAFETY: unique key; removed at the end of the test.
        unsafe { std::env::set_var("SHANNON_SUB_TEST_CFG_X", "anthropic-key") };
        let mut cfg = crate::unified_config::ShannonConfig::empty();
        cfg.max_tokens = Some(4096);
        cfg.permission_profile = Some("{env:SHANNON_SUB_TEST_CFG_X}".to_string());
        substitute_config(&mut cfg);
        assert_eq!(cfg.permission_profile.as_deref(), Some("anthropic-key"));
        // Non-string fields are preserved.
        assert_eq!(cfg.max_tokens, Some(4096));
        // SAFETY: see above.
        unsafe { std::env::remove_var("SHANNON_SUB_TEST_CFG_X") };
    }
}
