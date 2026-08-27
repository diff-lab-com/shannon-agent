//! `shannon --dump-config` — the explainable, provenance-annotated view of
//! layered configuration (§4.10 W3-2).
//!
//! The engine's real merge order (lowest → highest precedence, mirroring
//! [`crate::unified_config::ConfigBuilder::build`]) is:
//!
//! 1. `builtin` — engine baseline, all keys unset;
//! 2. `user-global` — `~/.shannon/config.toml`;
//! 3. `project` — `.shannon.toml`;
//! 4. `env-vars` — `SHANNON_*` environment variables;
//! 5. `connected` — `~/.shannon/providers.toml` (`/connect`);
//! 6. `cli-overlay` — flags from this invocation.
//!
//! Every entry inside a layer carries its value plus an `overridden_by`
//! annotation when a higher layer supplies a *different* value for the same
//! key — so "why is this setting what it is?" is answerable without reading
//! merge code.

use crate::unified_config::{LayerSnapshot, ShannonConfig};
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// Which environment variable feeds which top-level config key.
const ENV_SOURCES: &[(&str, &str)] = &[
    ("max_tokens", "SHANNON_MAX_TOKENS"),
    ("temperature", "SHANNON_TEMPERATURE"),
    ("timeout", "SHANNON_TIMEOUT"),
    ("debug", "SHANNON_DEBUG"),
    ("enable_tools", "SHANNON_ENABLE_TOOLS"),
    ("max_context_tokens", "SHANNON_MAX_CONTEXT_TOKENS"),
    (
        "provider_model",
        "SHANNON_MODEL / SHANNON_PROVIDER / SHANNON_BASE_URL",
    ),
    ("permission_profile", "SHANNON_PERMISSION_PROFILE"),
];

/// One entry: its value plus why it does or doesn't win.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DumpEntry {
    /// The layer's own serialized value for this key.
    pub value: Value,
    /// Nearest higher layer that sets the same key to a different value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overridden_by: Option<&'static str>,
    /// Environment variable feeding this entry (`env-vars` layer only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env_var: Option<&'static str>,
}

/// One layer of the ladder: source label, backing path, presence, entries.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DumpLayer {
    /// Provenance label (`user-global`, `project`, …).
    pub source: &'static str,
    /// Backing file when file-backed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Did this layer contribute content?
    pub present: bool,
    /// Top-level keys in deterministic (alphabetical) order.
    pub entries: BTreeMap<String, DumpEntry>,
}

/// The full dump: the ordered layer ladder plus the merged effective config.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ConfigDump {
    /// Layer order equals precedence order, lowest first.
    pub layers: Vec<DumpLayer>,
    /// Post-merge effective configuration (what the engine actually runs).
    pub effective: Value,
}

/// Fold raw layers without post-processing clamps (test helper parity with
/// [`crate::unified_config::ConfigBuilder::build`] up to those clamps).
pub fn fold_layers(layers: &[ShannonConfig]) -> ShannonConfig {
    let mut acc = ShannonConfig::empty();
    for l in layers {
        acc = acc.merge(l);
    }
    acc
}

/// Keys whose top-level values count as "unset" (null/empty) in a layer map.
fn contributes(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
        _ => true,
    }
}

/// Serialize one layer's config to top-level entries, dropping keys that are
/// indistinguishable from the engine's all-unset baseline (`debug = false`,
/// empty `provider_model`, …) so absent settings cannot masquerade as
/// explicitly-authored ones. Trade-off noted in ECOSYSTEM.md: a file that
/// *literally* writes the default value renders identically to not setting
/// it — provenance shows intent, byte-exact echoes stay out.
fn layer_entries(
    source: &str,
    config: &ShannonConfig,
    baseline: &Value,
) -> BTreeMap<String, DumpEntry> {
    let mut out = BTreeMap::new();
    let Ok(Value::Object(raw)) = serde_json::to_value(config) else {
        return out;
    };
    let Value::Object(baseline_obj) = baseline else {
        return out;
    };
    let env_backed = source == "env-vars";
    for (key, value) in raw {
        if !contributes(&value) {
            continue;
        }
        if baseline_obj.get(&key) == Some(&value) {
            continue;
        }
        let env_var = if env_backed {
            ENV_SOURCES
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, var)| *var)
        } else {
            None
        };
        out.insert(
            key,
            DumpEntry {
                value,
                overridden_by: None,
                env_var,
            },
        );
    }
    out
}

/// Build the dump from ordered snapshots plus the engine-merged effective
/// config (callers pass [`crate::unified_config::ConfigBuilder::build`]).
pub fn build_dump(layers: &[LayerSnapshot], effective: &ShannonConfig) -> ConfigDump {
    let baseline = serde_json::to_value(ShannonConfig::empty()).unwrap_or(Value::Null);
    // Materialize each layer's entries once.
    let materialized: Vec<DumpLayer> = layers
        .iter()
        .map(|l| DumpLayer {
            source: l.source,
            path: l.path.as_ref().map(|p| p.display().to_string()),
            present: l.present,
            entries: layer_entries(l.source, &l.config, &baseline),
        })
        .collect();

    // For every entry find the nearest higher layer that sets a *different*
    // value for the same key; annotate overridden_by accordingly.
    let mut annotated = materialized.clone();
    for i in 0..annotated.len() {
        let entries_snapshot: Vec<(String, Value)> = annotated[i]
            .entries
            .iter()
            .map(|(k, e)| (k.clone(), e.value.clone()))
            .collect();
        for (key, value) in entries_snapshot {
            let mut winner = None;
            // only strictly-higher-precedence layers can shadow this entry
            for higher in annotated.iter().skip(i + 1) {
                if let Some(e) = higher.entries.get(&key) {
                    if e.value != value {
                        winner = Some(higher.source);
                        break;
                    }
                }
            }
            if let Some(winner_src) = winner {
                if let Some(e) = annotated[i].entries.get_mut(&key) {
                    e.overridden_by = Some(winner_src);
                }
            }
        }
    }

    let effective_value = serde_json::to_value(effective).unwrap_or(Value::Object(Map::new()));

    ConfigDump {
        layers: annotated,
        effective: effective_value,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use insta::assert_json_snapshot;

    fn cfg(json: &str) -> ShannonConfig {
        serde_json::from_str(json).expect("synthetic layer parses")
    }

    fn snap(source: &'static str, path: Option<&str>, json: &str) -> LayerSnapshot {
        LayerSnapshot {
            source,
            path: path.map(std::path::PathBuf::from),
            present: true,
            config: cfg(json),
        }
    }

    /// Deterministic synthetic ladder used by both tests.
    fn fixture_layers() -> Vec<LayerSnapshot> {
        vec![
            LayerSnapshot::builtin(),
            snap(
                "user-global",
                Some("/home/alice/.shannon/config.toml"),
                r#"{"debug": true, "temperature": 0.25}"#,
            ),
            snap(
                "project",
                Some("/repo/.shannon.toml"),
                r#"{"temperature": 0.75}"#,
            ),
            snap("env-vars", None, r#"{"max_tokens": 4096, "debug": true}"#),
            LayerSnapshot {
                source: "connected",
                path: None,
                present: false,
                config: ShannonConfig::empty(),
            },
            snap("cli-overlay", None, r#"{"max_tokens": 8192}"#),
        ]
    }

    #[test]
    fn override_attribution_names_the_nearest_higher_layer() {
        let layers = fixture_layers();
        let merged = fold_layers(&layers.iter().map(|l| l.config.clone()).collect::<Vec<_>>());
        let dump = build_dump(&layers, &merged);

        // project temperature 0.7 is shadowed by nothing above it (env/cli unset)…
        let project = dump
            .layers
            .iter()
            .find(|l| l.source == "project")
            .expect("project layer present");
        assert_eq!(
            project.entries["temperature"].value,
            serde_json::json!(0.75)
        );
        assert_eq!(project.entries["temperature"].overridden_by, None);

        // user-global temperature IS shadowed by the project layer.
        let global = dump
            .layers
            .iter()
            .find(|l| l.source == "user-global")
            .expect("global layer");
        assert_eq!(global.entries["temperature"].overridden_by, Some("project"));
        // …but user-global debug survives: env repeats the same value, so it is
        // not an override.
        assert_eq!(global.entries["debug"].overridden_by, None);

        // env max_tokens loses to cli-overlay.
        let env = dump.layers.iter().find(|l| l.source == "env-vars").unwrap();
        assert_eq!(env.entries["max_tokens"].overridden_by, Some("cli-overlay"));
        assert_eq!(
            env.entries["max_tokens"].env_var,
            Some("SHANNON_MAX_TOKENS")
        );

        // effective follows real precedence: cli max_tokens + project temp.
        assert_eq!(dump.effective["max_tokens"], serde_json::json!(8192));
        assert_eq!(dump.effective["temperature"], serde_json::json!(0.75));
        assert_eq!(dump.effective["debug"], serde_json::json!(true));
    }

    /// Golden snapshot (§4.10 verification standard): pins layer ordering,
    /// provenance labels, path reporting and override annotations against
    /// silent drift.
    #[test]
    fn dump_config_golden_snapshot() {
        let layers = fixture_layers();
        let merged = fold_layers(&layers.iter().map(|l| l.config.clone()).collect::<Vec<_>>());
        let dump = build_dump(&layers, &merged);
        assert_json_snapshot!("dump_config_golden", dump);
    }

    #[test]
    fn unset_and_default_keys_are_omitted_from_layers() {
        let layers = vec![snap(
            "project",
            Some(".shannon.toml"),
            r#"{"presets": {}, "timeout": 30}"#,
        )];
        let dump = build_dump(&layers, &fold_layers(&[]));
        let project = &dump.layers[0];
        // empty container == unset -> hidden
        assert!(!project.entries.contains_key("presets"));
        // serde-default scalar (debug=false in an untouched file) -> hidden
        assert!(!project.entries.contains_key("debug"));
        // genuinely-set key survives
        assert_eq!(project.entries["timeout"].value, serde_json::json!(30));
    }
}
