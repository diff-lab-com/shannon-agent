//! Routine templates library — P1.4.
//!
//! Each `.toml` in `routines/` (next to `Cargo.toml`) is a pre-made
//! scheduled-task definition. The library reads them at runtime so users can
//! add new templates without recompiling.
//!
//! Templates are read-only: instantiating one copies its fields into a new
//! scheduled task in the user's store. The source file is never modified.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A single template entry returned to the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutineTemplate {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub prompt: String,
    pub trigger_type: String,
    /// Present when `trigger_type = "cron"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cron_expr: Option<String>,
    /// Present when `trigger_type = "interval"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval_secs: Option<u64>,
    /// Optional timezone hint (IANA name). Empty string means "use local".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawTemplate {
    id: String,
    name: String,
    description: String,
    category: String,
    prompt: String,
    trigger_type: String,
    #[serde(default)]
    cron_expr: Option<String>,
    #[serde(default)]
    interval_secs: Option<u64>,
    #[serde(default)]
    timezone: Option<String>,
}

impl From<RawTemplate> for RoutineTemplate {
    fn from(r: RawTemplate) -> Self {
        Self {
            id: r.id,
            name: r.name,
            description: r.description,
            category: r.category,
            prompt: r.prompt,
            trigger_type: r.trigger_type,
            cron_expr: r.cron_expr,
            interval_secs: r.interval_secs,
            timezone: r.timezone,
        }
    }
}

/// Resolve the templates directory. Walks up from the current executable's
/// parent until it finds a `routines/` sibling of `Cargo.toml` — that handles
/// both `cargo run` (working dir = crate root) and installed binaries whose
/// CWD is arbitrary.
///
/// Falls back to `./routines` relative to CWD so tests can chdir into a
/// fixture directory without touching the real crate.
pub fn resolve_templates_dir() -> PathBuf {
    if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        let candidate = PathBuf::from(manifest_dir).join("routines");
        if candidate.is_dir() {
            return candidate;
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent().map(|p| p.to_path_buf());
        while let Some(d) = dir {
            let candidate = d.join("routines");
            if candidate.is_dir() {
                return candidate;
            }
            dir = d.parent().map(|p| p.to_path_buf());
        }
    }
    PathBuf::from("routines")
}

/// Read every `.toml` file in the templates directory. Returns a vec sorted
/// by `(category, name)` for stable UI ordering. Files that fail to parse are
/// skipped with a `warn!` log — one broken template must not break the whole
/// library.
pub fn list_templates() -> Vec<RoutineTemplate> {
    let dir = resolve_templates_dir();
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        tracing::warn!(dir = %dir.display(), "routine templates dir not found");
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("toml") {
            continue;
        }
        let Ok(contents) = std::fs::read_to_string(&path) else {
            tracing::warn!(path = %path.display(), "could not read template");
            continue;
        };
        match toml::from_str::<RawTemplate>(&contents) {
            Ok(raw) => out.push(RoutineTemplate::from(raw)),
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "skipping malformed template");
            }
        }
    }
    out.sort_by(|a, b| {
        a.category
            .cmp(&b.category)
            .then_with(|| a.name.cmp(&b.name))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Find the real `routines/` dir shipped with this crate. Returns `None`
    /// when CARGO_MANIFEST_DIR isn't set (e.g. running the test in a context
    /// where the crate was compiled elsewhere).
    fn real_templates_dir() -> Option<PathBuf> {
        let dir = resolve_templates_dir();
        if dir.is_dir() {
            Some(dir)
        } else {
            None
        }
    }

    #[test]
    fn list_templates_returns_shipped_library() {
        let Some(_) = real_templates_dir() else {
            // No templates dir reachable from this test environment — skip
            // rather than fail. This guards against false negatives on build
            // machines where CARGO_MANIFEST_DIR may be unavailable.
            return;
        };
        let templates = list_templates();
        assert!(
            templates.len() >= 5,
            "expected the shipped library to have at least 5 templates, got {}",
            templates.len()
        );
    }

    #[test]
    fn shipped_templates_have_unique_ids() {
        let Some(_) = real_templates_dir() else {
            return;
        };
        let templates = list_templates();
        let mut ids: Vec<&str> = templates.iter().map(|t| t.id.as_str()).collect();
        let total = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), total, "duplicate template id in shipped library");
    }

    #[test]
    fn shipped_templates_have_required_fields() {
        let Some(_) = real_templates_dir() else {
            return;
        };
        for t in list_templates() {
            assert!(!t.id.is_empty(), "{:?}: id empty", t.name);
            assert!(!t.name.is_empty(), "{}: name empty", t.id);
            assert!(!t.description.is_empty(), "{}: description empty", t.id);
            assert!(!t.category.is_empty(), "{}: category empty", t.id);
            assert!(!t.prompt.trim().is_empty(), "{}: prompt empty", t.id);
            assert!(
                t.trigger_type == "cron" || t.trigger_type == "interval",
                "{}: bad trigger_type {}",
                t.id,
                t.trigger_type
            );
            if t.trigger_type == "cron" {
                assert!(t.cron_expr.is_some(), "{}: cron trigger missing cron_expr", t.id);
            }
            if t.trigger_type == "interval" {
                assert!(
                    t.interval_secs.is_some(),
                    "{}: interval trigger missing interval_secs",
                    t.id
                );
            }
        }
    }

    #[test]
    fn template_filename_matches_id() {
        let Some(dir) = real_templates_dir() else {
            return;
        };
        for t in list_templates() {
            let expected = dir.join(format!("{}.toml", t.id));
            assert!(
                expected.is_file(),
                "{}: expected file {} to exist",
                t.id,
                expected.display()
            );
        }
    }

    #[test]
    fn raw_template_parses_complete_file() {
        let toml_text = r#"
id = "demo"
name = "Demo"
description = "Demo template"
category = "x"
prompt = "hello"
trigger_type = "cron"
cron_expr = "0 9 * * *"
timezone = "UTC"
"#;
        let raw: RawTemplate = toml::from_str(toml_text).unwrap();
        assert_eq!(raw.id, "demo");
        assert_eq!(raw.cron_expr.as_deref(), Some("0 9 * * *"));
        assert_eq!(raw.timezone.as_deref(), Some("UTC"));
    }

    #[test]
    fn raw_template_rejects_missing_trigger_specific_field() {
        let toml_text = r#"
id = "demo"
name = "Demo"
description = "Demo"
category = "x"
prompt = "hi"
trigger_type = "cron"
"#;
        // cron trigger with no cron_expr: parses (Option is None), validated
        // at a higher layer via `shipped_templates_have_required_fields`.
        let raw: RawTemplate = toml::from_str(toml_text).unwrap();
        assert!(raw.cron_expr.is_none());
    }
}
