// J1-J4 Sprint 4 module: the public surface is wired into the CLI binary
// in a follow-up PR (clap subcommand plumbing). Until that lands, suppress
// the dead-code warnings that clippy::workspace fires for fully-typed
// helper modules that aren't yet reached from `main.rs`.
#![allow(dead_code)]

//! CLI surface for `/triggered` — inspect and toggle hook-triggered routines.
//!
//! These routines (loaded from `.shannon/routines.toml` and
//! `.claude/routines.toml`) fire automatically when matching hook events
//! occur. They complement the cron-style [`shannon_core::scheduled_routines`]
//! system with event-driven automation.
//!
//! ## Subcommands
//!
//! - `shannon triggered list`             — print every routine
//! - `shannon triggered enable <name>`    — flip a routine on
//! - `shannon triggered disable <name>`   — flip a routine off
//! - `shannon triggered show <name>`      — print one routine in full
//!
//! ## Editing Model
//!
//! We do **not** rewrite the source TOML in place — the user may have
//! formatted it carefully. Instead, a per-routine enable/disable flag is
//! maintained in `~/.shannon/triggered_overrides.toml`. The registry's
//! loader reads both files and applies overrides, so toggling a routine
//! is reversible and survives TOML reformats.

use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use shannon_core::triggered_routines::{TriggeredRoutineDef, TriggeredRoutineRegistry};

/// File holding user-applied enable/disable overrides.
#[allow(dead_code)] // KEEP: public API for future CLI flags
pub const OVERRIDES_FILE: &str = "triggered_overrides.toml";

/// Where the overrides file lives (per-user).
#[allow(dead_code)] // KEEP: public API for future CLI flags
pub fn default_overrides_path() -> PathBuf {
    if let Some(home) = dirs::home_dir() {
        return home.join(".shannon").join(OVERRIDES_FILE);
    }
    PathBuf::from(OVERRIDES_FILE)
}

/// Per-name enable/disable override applied on top of the registry.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct OverrideStore {
    /// Map of routine name → enabled flag. Missing entries mean "no override".
    #[serde(default)]
    pub flags: HashMap<String, bool>,
}

impl OverrideStore {
    /// Load from `path`, returning an empty store on missing-file (a fresh
    /// install has no overrides yet).
    pub fn load(path: &PathBuf) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading overrides file {}", path.display()))?;
        let store: Self = toml::from_str(&text)
            .with_context(|| format!("parsing overrides file {}", path.display()))?;
        Ok(store)
    }

    /// Persist to `path`. Creates parent dirs as needed.
    pub fn save(&self, path: &PathBuf) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating parent dir {}", parent.display()))?;
        }
        let text = toml::to_string_pretty(self).context("serializing overrides")?;
        std::fs::write(path, text)
            .with_context(|| format!("writing overrides file {}", path.display()))?;
        Ok(())
    }

    /// Returns the override for `name`, if any.
    pub fn get(&self, name: &str) -> Option<bool> {
        self.flags.get(name).copied()
    }

    /// Set the override for `name`.
    pub fn set(&mut self, name: &str, enabled: bool) {
        self.flags.insert(name.to_string(), enabled);
    }

    /// Clear the override for `name` (revert to file-defined enabled state).
    #[allow(dead_code)] // KEEP: public API for future reset command
    pub fn clear(&mut self, name: &str) {
        self.flags.remove(name);
    }
}

/// A routine as observed by the CLI: source definition + effective enabled flag.
#[derive(Debug, Clone)]
pub struct ResolvedRoutine {
    pub def: TriggeredRoutineDef,
    pub effective_enabled: bool,
    pub override_active: bool,
}

/// Resolve registry + overrides into a flat list of [`ResolvedRoutine`],
/// sorted alphabetically by name for deterministic CLI output.
pub fn resolve(
    registry: &TriggeredRoutineRegistry,
    overrides: &OverrideStore,
) -> Vec<ResolvedRoutine> {
    let mut out: Vec<ResolvedRoutine> = registry
        .all()
        .values()
        .map(|def| {
            let override_active = overrides.flags.contains_key(&def.name);
            let effective_enabled = overrides.get(&def.name).unwrap_or(def.enabled);
            ResolvedRoutine {
                def: def.clone(),
                effective_enabled,
                override_active,
            }
        })
        .collect();
    out.sort_by(|a, b| a.def.name.cmp(&b.def.name));
    out
}

/// CLI subcommand verb.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggeredCommand {
    List,
    Enable(String),
    Disable(String),
    Show(String),
}

impl FromStr for TriggeredCommand {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        let mut parts = s.split_whitespace();
        let verb = parts
            .next()
            .ok_or_else(|| anyhow::anyhow!("missing verb (list|enable|disable|show)"))?;
        let mut arg = || {
            parts
                .next()
                .ok_or_else(|| anyhow::anyhow!("missing routine name"))
        };
        match verb {
            "list" | "ls" => Ok(TriggeredCommand::List),
            "enable" | "on" => Ok(TriggeredCommand::Enable(arg()?.to_string())),
            "disable" | "off" => Ok(TriggeredCommand::Disable(arg()?.to_string())),
            "show" => Ok(TriggeredCommand::Show(arg()?.to_string())),
            other => anyhow::bail!("unknown verb '{other}' (expected list|enable|disable|show)"),
        }
    }
}

/// Apply a [`TriggeredCommand`] to the registry + overrides, returning
/// a human-readable result string for the caller to print.
pub fn apply(
    registry: &TriggeredRoutineRegistry,
    overrides: &mut OverrideStore,
    cmd: &TriggeredCommand,
) -> Result<TriggeredOutput> {
    match cmd {
        TriggeredCommand::List => Ok(TriggeredOutput::List(resolve(registry, overrides))),
        TriggeredCommand::Show(name) => {
            let resolved = resolve(registry, overrides);
            match resolved.into_iter().find(|r| r.def.name == *name) {
                Some(r) => Ok(TriggeredOutput::Show(r)),
                None => anyhow::bail!("no routine named '{name}'"),
            }
        }
        TriggeredCommand::Enable(name) => {
            ensure_exists(registry, name)?;
            overrides.set(name, true);
            Ok(TriggeredOutput::Toggled {
                name: name.clone(),
                enabled: true,
            })
        }
        TriggeredCommand::Disable(name) => {
            ensure_exists(registry, name)?;
            overrides.set(name, false);
            Ok(TriggeredOutput::Toggled {
                name: name.clone(),
                enabled: false,
            })
        }
    }
}

fn ensure_exists(registry: &TriggeredRoutineRegistry, name: &str) -> Result<()> {
    if registry.get(name).is_none() {
        anyhow::bail!("no routine named '{name}'");
    }
    Ok(())
}

/// CLI output variants.
#[derive(Debug, Clone)]
pub enum TriggeredOutput {
    List(Vec<ResolvedRoutine>),
    Show(ResolvedRoutine),
    Toggled { name: String, enabled: bool },
}

/// Format [`TriggeredOutput`] as plain text for stdout.
pub fn format_output(out: &TriggeredOutput) -> String {
    match out {
        TriggeredOutput::List(rs) => {
            if rs.is_empty() {
                return "no triggered routines".into();
            }
            let mut s =
                String::from("name                          enabled  trigger          matcher\n");
            for r in rs {
                let en = if r.effective_enabled { "yes" } else { "no" };
                let en: String = if r.override_active {
                    format!("{en}*")
                } else {
                    en.to_string()
                };
                let matcher = r.def.matcher.as_deref().unwrap_or("-");
                s.push_str(&format!(
                    "{name:<28} {en:<8}  {trigger:<15} {matcher}\n",
                    name = r.def.name,
                    trigger = r.def.trigger,
                    matcher = matcher,
                ));
            }
            s
        }
        TriggeredOutput::Show(r) => {
            let mut s = format!("name:    {}\n", r.def.name);
            s.push_str(&format!(
                "enabled: {}{}\n",
                r.effective_enabled,
                if r.override_active { " (override)" } else { "" }
            ));
            s.push_str(&format!("trigger: {}\n", r.def.trigger));
            if let Some(m) = &r.def.matcher {
                s.push_str(&format!("matcher: {m}\n"));
            }
            if let Some(p) = &r.def.pattern {
                s.push_str(&format!("pattern: {p}\n"));
            }
            s.push_str(&format!("command: {}\n", r.def.command));
            s
        }
        TriggeredOutput::Toggled { name, enabled } => {
            format!(
                "routine {name} {}",
                if *enabled { "enabled" } else { "disabled" }
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shannon_core::triggered_routines::TriggeredRoutineRegistry;

    /// Build a registry by writing a temporary TOML file and loading it.
    /// `TriggeredRoutineRegistry::routines` is private, so tests can't seed
    /// it directly — the TOML round-trip is the supported entry point.
    fn build_registry() -> TriggeredRoutineRegistry {
        let tmp = tempfile::tempdir().unwrap();
        let toml = r#"
[[routine]]
name = "post-edit-lint"
trigger = "PostToolUse"
matcher = "Edit|Write"
command = "cargo clippy --fix --allow-dirty"
enabled = true

[[routine]]
name = "session-pull"
trigger = "SessionStart"
command = "git pull --rebase"
enabled = false
"#;
        let path = tmp.path().join("routines.toml");
        std::fs::write(&path, toml).unwrap();
        let mut reg = TriggeredRoutineRegistry::new();
        reg.load_from_file(&path);
        reg
    }

    #[test]
    fn override_store_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("overrides.toml");

        let mut store = OverrideStore::default();
        store.set("a", true);
        store.set("b", false);
        store.save(&path).unwrap();

        let loaded = OverrideStore::load(&path).unwrap();
        assert_eq!(loaded, store);
    }

    #[test]
    fn override_store_missing_file_is_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("does_not_exist.toml");
        let store = OverrideStore::load(&path).unwrap();
        assert!(store.flags.is_empty());
    }

    #[test]
    fn resolve_applies_overrides() {
        let reg = build_registry();
        let mut store = OverrideStore::default();
        store.set("post-edit-lint", false);

        let resolved = resolve(&reg, &store);
        let r = resolved
            .iter()
            .find(|r| r.def.name == "post-edit-lint")
            .unwrap();
        assert!(!r.effective_enabled);
        assert!(r.override_active);

        let r = resolved
            .iter()
            .find(|r| r.def.name == "session-pull")
            .unwrap();
        assert!(!r.effective_enabled);
        assert!(!r.override_active);
    }

    #[test]
    fn resolve_sorts_alphabetically() {
        let reg = build_registry();
        let store = OverrideStore::default();
        let resolved = resolve(&reg, &store);
        assert_eq!(resolved[0].def.name, "post-edit-lint");
        assert_eq!(resolved[1].def.name, "session-pull");
    }

    #[test]
    fn from_str_parses_verbs() {
        assert_eq!(
            TriggeredCommand::from_str("list").unwrap(),
            TriggeredCommand::List
        );
        assert_eq!(
            TriggeredCommand::from_str("enable foo").unwrap(),
            TriggeredCommand::Enable("foo".into())
        );
        assert_eq!(
            TriggeredCommand::from_str("disable bar").unwrap(),
            TriggeredCommand::Disable("bar".into())
        );
        assert_eq!(
            TriggeredCommand::from_str("show baz").unwrap(),
            TriggeredCommand::Show("baz".into())
        );
    }

    #[test]
    fn from_str_rejects_unknown_verbs() {
        assert!(TriggeredCommand::from_str("").is_err());
        assert!(TriggeredCommand::from_str("nope").is_err());
        assert!(TriggeredCommand::from_str("enable").is_err()); // missing name
    }

    #[test]
    fn apply_enable_writes_override() {
        let reg = build_registry();
        let mut store = OverrideStore::default();
        apply(
            &reg,
            &mut store,
            &TriggeredCommand::Enable("session-pull".into()),
        )
        .unwrap();
        assert_eq!(store.get("session-pull"), Some(true));
    }

    #[test]
    fn apply_disable_writes_override() {
        let reg = build_registry();
        let mut store = OverrideStore::default();
        apply(
            &reg,
            &mut store,
            &TriggeredCommand::Disable("post-edit-lint".into()),
        )
        .unwrap();
        assert_eq!(store.get("post-edit-lint"), Some(false));
    }

    #[test]
    fn apply_unknown_routine_errors() {
        let reg = build_registry();
        let mut store = OverrideStore::default();
        assert!(apply(&reg, &mut store, &TriggeredCommand::Enable("nope".into())).is_err());
    }

    #[test]
    fn format_list_empty() {
        let s = format_output(&TriggeredOutput::List(vec![]));
        assert_eq!(s, "no triggered routines");
    }

    #[test]
    fn format_list_with_rows() {
        let reg = build_registry();
        let store = OverrideStore::default();
        let out = format_output(&TriggeredOutput::List(resolve(&reg, &store)));
        assert!(out.contains("post-edit-lint"));
        assert!(out.contains("session-pull"));
    }

    #[test]
    fn format_toggled() {
        let out = format_output(&TriggeredOutput::Toggled {
            name: "x".into(),
            enabled: true,
        });
        assert_eq!(out, "routine x enabled");
    }
}
