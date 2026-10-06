//! `shannon config --explain <key>` — the human-readable, one-key view of
//! layered configuration provenance (R4-4a; roadmap decision ③ read-only
//! part — this module never writes anything).
//!
//! The engine's merge order (lowest → highest precedence, mirroring
//! [`shannon_core::unified_config::ConfigBuilder::build`]) is:
//! builtin → user-global (`~/.shannon/config.toml`) → project
//! (`.shannon.toml`) → env-vars (`SHANNON_*`) → connected
//! (`~/.shannon/providers.toml`) → cli-overlay (this invocation's flags).
//!
//! Provenance is **reused** from `--dump-config`
//! ([`shannon_core::config_dump`]): the layer snapshots already record which
//! layer contributes which top-level key, so `--explain` renders the same
//! ladder for a single key instead of maintaining a second provenance
//! implementation. The JSON dump stays the machine surface; this module is
//! the human one.
//!
//! Key resolution:
//! - scalar ShannonConfig keys (`max_tokens`, `temperature`, …) are looked up
//!   per layer directly;
//! - `model` / `provider` / `base_url` are v2 aliases: they live inside the
//!   `provider_model` key of each layer, so the render descends into each
//!   layer's active target (this is what "model → connected layer" means);
//! - desktop-only keys (`plan_tier`, `approval_mode`, … — P-N18) get the
//!   desktop answer: they live in the desktop's own `config.json`, which the
//!   engine never reads, so there is no layer ladder to render;
//! - credential-shaped names (contains `api_key`, `token`, …) get the A1
//!   answer: secrets are never part of the layered config — `/connect`;
//! - anything else prints the known-key list.

use anyhow::Result;
use serde_json::Value;
use shannon_core::config_dump::{DumpEntry, DumpLayer};

/// Every known config key with its "where to change it" hint. Scalar names
/// are the serde names of
/// [`shannon_core::unified_config::ShannonConfig`] — the same top-level keys
/// `shannon --dump-config` prints per layer.
const KNOWN_KEYS: &[(&str, &str)] = &[
    (
        "debug",
        "`shannon config debug=true|false` (writes ~/.shannon/config.toml), SHANNON_DEBUG, or --debug for one invocation",
    ),
    (
        "enable_tools",
        "`enable_tools = true|false` in ~/.shannon/config.toml or .shannon.toml, or SHANNON_ENABLE_TOOLS",
    ),
    (
        "hooks",
        "a `[hooks]` table in ~/.shannon/config.toml or .shannon.toml",
    ),
    (
        "max_context_tokens",
        "`max_context_tokens = <n>` in ~/.shannon/config.toml or .shannon.toml, or SHANNON_MAX_CONTEXT_TOKENS",
    ),
    (
        "max_tokens",
        "`shannon config max_tokens=<n>` (writes ~/.shannon/config.toml), SHANNON_MAX_TOKENS, or --max-tokens for one invocation",
    ),
    (
        "notifications",
        "a `[notifications]` table in ~/.shannon/config.toml or .shannon.toml",
    ),
    (
        "permission_profile",
        "`shannon config permission_profile=<name>`, SHANNON_PERMISSION_PROFILE, or --permission-mode for one invocation",
    ),
    (
        "presets",
        "`[presets.*]` tables in ~/.shannon/config.toml or .shannon.toml",
    ),
    (
        "provider_model",
        "the connected provider profile — change it with /connect, `shannon providers add`, or `/model --save` (writes ~/.shannon/providers.toml)",
    ),
    (
        "temperature",
        "`shannon config temperature=<0.0-2.0>` (writes ~/.shannon/config.toml) or SHANNON_TEMPERATURE",
    ),
    (
        "timeout",
        "`shannon config timeout=<seconds>` (writes ~/.shannon/config.toml) or SHANNON_TIMEOUT",
    ),
];

/// v2 aliases: user-facing names that live *inside* the per-layer
/// `provider_model` value rather than as top-level keys. `model`/`provider`
/// read the active target; `base_url` reads the active provider slot.
const MODEL_ALIASES: &[(&str, &str)] = &[
    (
        "model",
        "/model (REPL), `shannon config model=<model>` (writes ~/.shannon/config.toml), --model for one invocation, or SHANNON_MODEL",
    ),
    (
        "provider",
        "/connect (REPL), `shannon providers add`, `shannon config provider=<provider>`, or SHANNON_PROVIDER",
    ),
    (
        "base_url",
        "/connect (REPL) or `shannon providers add --base-url <url>` (writes ~/.shannon/providers.toml)",
    ),
];

/// The desktop's own config file. This is the desktop crate's `config_path()`
/// (`~/.shannon/desktop/config.json`, `desktop/src/config.rs`) — the constant
/// lives in the desktop crate and is private, so the CLI states the resolved
/// path here. Read-only: `--explain` never writes it.
const DESKTOP_CONFIG_PATH: &str = "~/.shannon/desktop/config.json";

/// Desktop-UI-only keys (P-N18): they are persisted in the *desktop's* own
/// config file ([`DESKTOP_CONFIG_PATH`]) and the engine's layered config
/// never reads them — so unlike the keys above there is no layer ladder to
/// render, only a pointer at the Settings surface that writes each one.
const DESKTOP_KEYS: &[(&str, &str)] = &[
    (
        "plan_tier",
        "desktop Settings → Models → Plan/Act phase tiers (Plan row)",
    ),
    (
        "act_tier",
        "desktop Settings → Models → Plan/Act phase tiers (Act row)",
    ),
    (
        "effort_level",
        "the chat composer's model menu (reasoning-effort sub-tier)",
    ),
    (
        "approval_mode",
        "desktop Settings → General → approval mode (or the composer's execution-mode switcher)",
    ),
    (
        "enabled_providers",
        "desktop Settings → Models → provider visibility",
    ),
];

/// Build the layer dump for `--explain`, exactly like `shannon --dump-config`
/// does (same loaders, same overlay reconstruction from this invocation's
/// flags) so the two surfaces can never disagree about provenance.
pub fn build_explain_dump(
    cli_model: Option<&str>,
    cli_provider: Option<&str>,
) -> shannon_core::config_dump::ConfigDump {
    let mut builder = shannon_core::unified_config::ConfigBuilder::new();
    builder.load_global_toml();
    builder.load_local_toml();
    builder.load_env_vars();
    builder.load_connected_profile();
    let mut overlay = shannon_core::unified_config::ShannonConfig::empty();
    if cli_model.is_some() || cli_provider.is_some() {
        overlay.provider_model = shannon_core::provider_resolver::synthesize_default_profile(
            cli_model,
            cli_provider,
            None,
            None,
        )
        .unwrap_or_default();
    }
    builder.set_cli_overrides(overlay);
    shannon_core::config_dump::build_dump(&builder.layer_snapshots(), &builder.build())
}

/// Entry point behind `shannon config --explain <key>`: returns the full
/// human-readable explanation (print it; never JSON).
pub fn run(cli_model: Option<&str>, cli_provider: Option<&str>, key: &str) -> Result<String> {
    let dump = build_explain_dump(cli_model, cli_provider);
    Ok(render_explain(key, &dump))
}

/// Compact display of a JSON value: bare for strings, compact JSON otherwise.
fn display_value(v: &Value) -> String {
    match v.as_str() {
        Some(s) => s.to_string(),
        None => serde_json::to_string(v).unwrap_or_else(|_| "<unprintable>".to_string()),
    }
}

/// One resolved key: what the render should look at.
enum ExplainTarget {
    /// A direct ShannonConfig top-level key.
    Scalar(&'static str),
    /// A field inside each layer's `provider_model` value (`model`,
    /// `provider`, `base_url`).
    ModelAlias(&'static str),
    /// A desktop-UI-only key (P-N18) — no layer ladder, the desktop answer.
    DesktopKey(&'static str),
    /// A credential-shaped name — gets the A1 answer.
    Credential,
}

/// Resolve the user's key to an [`ExplainTarget`], or `None` (with the known
/// keys) when unrecognized. The known-key allowlists are consulted FIRST —
/// the same order `shannon config <key>=<value>` uses — because the
/// secret-shape predicate is coarse (`max_tokens` contains "token") and the
/// real keys are non-secret by definition.
fn resolve_key(key: &str) -> (Option<ExplainTarget>, Vec<&'static str>) {
    let trimmed = key.trim();
    if let Some((name, _)) = MODEL_ALIASES
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(trimmed))
    {
        return (Some(ExplainTarget::ModelAlias(name)), Vec::new());
    }
    if let Some((name, _)) = KNOWN_KEYS
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(trimmed))
    {
        return (Some(ExplainTarget::Scalar(name)), Vec::new());
    }
    if let Some((name, _)) = DESKTOP_KEYS
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(trimmed))
    {
        return (Some(ExplainTarget::DesktopKey(name)), Vec::new());
    }
    if shannon_core::config_persist::is_secret_shaped_key(trimmed) {
        return (Some(ExplainTarget::Credential), Vec::new());
    }
    let mut known: Vec<&'static str> = KNOWN_KEYS.iter().map(|(k, _)| *k).collect();
    known.extend(MODEL_ALIASES.iter().map(|(k, _)| *k));
    known.extend(DESKTOP_KEYS.iter().map(|(k, _)| *k));
    known.sort_unstable();
    (None, known)
}

/// The hint for a resolved key name (scalar or alias).
fn hint_for(name: &str) -> &'static str {
    KNOWN_KEYS
        .iter()
        .chain(MODEL_ALIASES.iter())
        .find(|(k, _)| *k == name)
        .map(|(_, h)| *h)
        .unwrap_or("see `shannon config --help`")
}

/// The Settings surface that writes a desktop-only key.
fn desktop_hint_for(name: &str) -> &'static str {
    DESKTOP_KEYS
        .iter()
        .find(|(k, _)| *k == name)
        .map(|(_, h)| *h)
        .unwrap_or("see the desktop Settings")
}

/// Best-effort, read-only peek at the desktop's config for the current value
/// of a desktop-only key. Absent file, unparsable JSON, or missing/null key
/// → `None` (the desktop answer never depends on it). The CLI never writes
/// this file. Path resolution mirrors the desktop's own `config_path()`
/// (`HOME` → `~/.shannon/desktop/config.json`) exactly.
fn read_desktop_config_value(name: &str) -> Option<Value> {
    let path = dirs::home_dir()?
        .join(".shannon")
        .join("desktop")
        .join("config.json");
    let text = std::fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    let v = value.get(name)?;
    if v.is_null() { None } else { Some(v.clone()) }
}

/// Where a layer's value comes from: the env var name for the env layer, the
/// backing file for file-backed layers, a note for the CLI overlay.
fn layer_origin(layer: &DumpLayer, entry: Option<&DumpEntry>) -> String {
    if layer.source == "env-vars" {
        return entry
            .and_then(|e| e.env_var)
            .unwrap_or("SHANNON_* environment")
            .to_string();
    }
    if layer.source == "cli-overlay" {
        return "this invocation's flags".to_string();
    }
    match &layer.path {
        Some(p) => p.clone(),
        None => "(no backing file)".to_string(),
    }
}

/// Origin for a model-alias entry: the dump records provenance at
/// `provider_model` granularity (whose ENV_SOURCES entry is the combined
/// `SHANNON_MODEL / SHANNON_PROVIDER / SHANNON_BASE_URL`), but for a single
/// alias the specific variable is the actionable answer.
fn alias_env_var(field: &str) -> Option<&'static str> {
    match field {
        "model" => Some("SHANNON_MODEL"),
        "provider" => Some("SHANNON_PROVIDER"),
        "base_url" => Some("SHANNON_BASE_URL"),
        _ => None,
    }
}

/// Origin of `layer`'s contribution to `entry_key`, resolving the specific
/// env var when a model alias is being explained.
fn origin_for(target: &ExplainTarget, layer: &DumpLayer, entry_key: &str) -> String {
    if layer.source == "env-vars"
        && let ExplainTarget::ModelAlias(field) = target
        && let Some(var) = alias_env_var(field)
    {
        return var.to_string();
    }
    layer_origin(layer, layer.entries.get(entry_key))
}

/// Extract `field` ("model" | "provider" | "base_url") from one layer's
/// serialized `provider_model` value: the active profile's active target
/// (and, for `base_url`, the matching provider slot). `None` when the layer
/// carries no connected profile or the field is blank.
fn provider_model_field(pm: &Value, field: &str) -> Option<Value> {
    let profiles = pm.get("profiles")?;
    let active = pm
        .get("active_profile")
        .and_then(|v| v.as_str())
        .unwrap_or("default");
    let profile = profiles.get(active)?;
    let target = profile.get("active_target")?;
    let non_empty = |v: Option<&Value>| -> Option<Value> {
        v.and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .map(|s| Value::String(s.to_string()))
    };
    match field {
        "model" => non_empty(target.get("model_id")),
        "provider" => non_empty(target.get("provider_id")),
        "base_url" => {
            let pid = target
                .get("provider_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            profile
                .get("providers")?
                .as_array()?
                .iter()
                .find(|p| p.get("id").and_then(|v| v.as_str()) == Some(pid))
                .and_then(|p| non_empty(p.get("base_url")))
        }
        _ => None,
    }
}

/// Collect `(layer, value)` pairs that define the requested key, lowest →
/// highest precedence. Scalars read the top-level entry; model aliases
/// descend into each layer's `provider_model`.
fn defining_layers<'a>(
    target: &ExplainTarget,
    layers: &'a [DumpLayer],
) -> Vec<(&'a DumpLayer, Value)> {
    let mut out = Vec::new();
    for layer in layers {
        if layer.source == "builtin" {
            continue; // the baseline never contributes a value
        }
        match target {
            ExplainTarget::Scalar(key) => {
                if let Some(entry) = layer.entries.get(*key) {
                    out.push((layer, entry.value.clone()));
                }
            }
            ExplainTarget::ModelAlias(field) => {
                if let Some(pm) = layer.entries.get("provider_model")
                    && let Some(v) = provider_model_field(&pm.value, field)
                {
                    out.push((layer, v));
                }
            }
            ExplainTarget::DesktopKey(_) => {}
            ExplainTarget::Credential => {}
        }
    }
    out
}

/// Render the explanation for `key` against `dump`. Human-readable text with
/// one fact per line: the layer ladder (origin + value per defining layer),
/// the winner, the change-it hint; the desktop answer for desktop-only keys;
/// the known-key list for an unknown key; or the A1 credentials answer for
/// secret-shaped names.
pub fn render_explain(key: &str, dump: &shannon_core::config_dump::ConfigDump) -> String {
    let (target, known) = resolve_key(key);
    let Some(target) = target else {
        return render_unknown(key, &known);
    };
    match target {
        ExplainTarget::Credential => render_credentials_answer(key),
        ExplainTarget::DesktopKey(name) => {
            let current = read_desktop_config_value(name);
            render_desktop_key(key, name, current.as_ref())
        }
        _ => render_layered(key, &target, dump),
    }
}

/// The desktop-only-key answer (P-N18): these keys live in the desktop's own
/// config file, which the engine never reads — so there is no layer ladder,
/// only where the value is stored and which Settings surface writes it. The
/// same answer whether or not the desktop has the key configured; a current
/// value is appended (read-only, best-effort) when one is present.
fn render_desktop_key(key: &str, name: &str, current: Option<&Value>) -> String {
    let mut out = String::new();
    out.push_str(&format!("key: {}\n", key.trim()));
    out.push_str(&format!(
        "  '{name}' is a desktop-UI-only key (桌面 UI 专属键): the engine never reads it,\n\
         \x20 so it has no layer in the config ladder.\n"
    ));
    out.push_str(&format!(
        "  stored in: {DESKTOP_CONFIG_PATH} (the desktop's own config file)\n"
    ));
    if let Some(value) = current {
        out.push_str(&format!(
            "  current desktop value: {} (read-only peek at {DESKTOP_CONFIG_PATH})\n",
            display_value(value)
        ));
    }
    out.push_str(&format!("  change it: {}\n", desktop_hint_for(name)));
    out
}

/// The layered answer: ladder + winner + hint.
fn render_layered(
    key: &str,
    target: &ExplainTarget,
    dump: &shannon_core::config_dump::ConfigDump,
) -> String {
    let layers = defining_layers(target, &dump.layers);
    let canonical = match target {
        ExplainTarget::Scalar(k) | ExplainTarget::ModelAlias(k) => *k,
        ExplainTarget::DesktopKey(_) => {
            unreachable!("desktop keys are answered by render_desktop_key, not the layer ladder")
        }
        ExplainTarget::Credential => unreachable!("credential target never reaches here"),
    };
    let mut out = String::new();
    out.push_str(&format!("key: {}\n", key.trim()));
    if layers.is_empty() {
        out.push_str(&format!(
            "  no config layer defines {canonical} — the runtime default applies\n"
        ));
        out.push_str(&format!("  change it: {}\n", hint_for(canonical)));
        return out;
    }

    // Highest-precedence defining layer wins (the ladder is ordered lowest
    // first); earlier entries are the "losers" the value overrides.
    let (winner_layer, winner_value) = layers.last().expect("non-empty above");
    // The dump keys entries by ShannonConfig field; model aliases live
    // inside the `provider_model` entry of each layer.
    let entry_key = match target {
        ExplainTarget::Scalar(k) => *k,
        ExplainTarget::ModelAlias(_) => "provider_model",
        ExplainTarget::DesktopKey(_) => {
            unreachable!("desktop keys are answered by render_desktop_key, not the layer ladder")
        }
        ExplainTarget::Credential => unreachable!("credential target never reaches here"),
    };
    out.push_str("  defined by (lowest → highest precedence):\n");
    for (layer, value) in &layers {
        out.push_str(&format!(
            "    {} — {}: {}\n",
            layer.source,
            origin_for(target, layer, entry_key),
            display_value(value)
        ));
    }
    out.push_str(&format!(
        "  winner: {} ({}) = {}\n",
        winner_layer.source,
        origin_for(target, winner_layer, entry_key),
        display_value(winner_value)
    ));
    out.push_str(&format!("  change it: {}\n", hint_for(canonical)));
    out
}

/// The A1 credentials answer: layered config never carries secrets.
fn render_credentials_answer(key: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!("key: {}\n", key.trim()));
    out.push_str(
        "  credentials are not part of the layered config (decision A1: no plaintext in any\n\
         \x20 config file). Keys live in the credential store\n\
         \x20 (~/.shannon/credentials/<service>.json, mode 0600) or in an environment\n\
         \x20 variable; providers.toml carries only references (env:<VAR> / store:<service>).\n",
    );
    out.push_str(
        "  change it: /connect (REPL), the desktop provider settings, or\n\
         \x20 `shannon providers keys <provider> add env:VAR|store:SVC`\n",
    );
    out
}

/// The unknown-key answer: say so, list every known key, point at the JSON
/// dump for the full picture.
fn render_unknown(key: &str, known: &[&'static str]) -> String {
    let mut out = String::new();
    out.push_str(&format!("unknown config key: '{}'\n\n", key.trim()));
    out.push_str("known keys:\n");
    for k in known {
        out.push_str(&format!("  {k}\n"));
    }
    out.push_str(
        "\nnames containing \"api_key\", \"token\", \"secret\", \"password\" or \"credential\"\n\
         \x20 explain where credentials live instead.\n\
         \x20tip: `shannon --dump-config` prints the full layered provenance as JSON.\n",
    );
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use shannon_core::config_dump::{build_dump, fold_layers};
    use shannon_core::unified_config::{LayerSnapshot, ShannonConfig};

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

    /// Synthetic ladder: global sets max_tokens + temperature, env overrides
    /// max_tokens, the connected layer carries a model, the CLI overlay
    /// overrides temperature.
    fn fixture_dump() -> shannon_core::config_dump::ConfigDump {
        let connected_pm = r#"{"provider_model": {"version": 2, "profiles": {"default": {
            "name": "default",
            "active_target": {"provider_id": "glm", "model_id": "glm-4.6", "scope": "global"},
            "providers": [{"id": "glm", "kind": "openai-compatible", "display_name": "glm",
                           "base_url": "https://open.bigmodel.cn/v1",
                           "credential": {"backend": "store", "service": "glm"}}]}}}}"#;
        let cli_pm = r#"{"provider_model": {"version": 2, "profiles": {"default": {
            "name": "default",
            "active_target": {"provider_id": "openai", "model_id": "gpt-4o", "scope": "global"},
            "providers": []}}}}"#;
        let layers = vec![
            LayerSnapshot::builtin(),
            snap(
                "user-global",
                Some("/home/alice/.shannon/config.toml"),
                r#"{"max_tokens": 4096, "temperature": 0.25}"#,
            ),
            snap("project", Some("/repo/.shannon.toml"), r#"{"debug": true}"#),
            snap("env-vars", None, r#"{"max_tokens": 8192}"#),
            snap(
                "connected",
                Some("/home/alice/.shannon/providers.toml"),
                connected_pm,
            ),
            snap("cli-overlay", None, cli_pm),
        ];
        let merged = fold_layers(&layers.iter().map(|l| l.config.clone()).collect::<Vec<_>>());
        build_dump(&layers, &merged)
    }

    #[test]
    fn scalar_winner_is_highest_layer_and_losers_are_listed() {
        let out = render_explain("max_tokens", &fixture_dump());
        assert!(out.starts_with("key: max_tokens"), "{out}");
        assert!(
            out.contains("user-global — /home/alice/.shannon/config.toml: 4096"),
            "loser layer with path + value must be listed: {out}"
        );
        assert!(
            out.contains("env-vars — SHANNON_MAX_TOKENS: 8192"),
            "env layer must name the env var: {out}"
        );
        assert!(
            out.contains("winner: env-vars (SHANNON_MAX_TOKENS) = 8192"),
            "winner must be the highest layer: {out}"
        );
        assert!(
            out.contains("change it:"),
            "every layered answer ends with the change-it hint: {out}"
        );
    }

    #[test]
    fn model_alias_descends_into_provider_model_per_layer() {
        let out = render_explain("model", &fixture_dump());
        assert!(
            out.contains("connected — /home/alice/.shannon/providers.toml: glm-4.6"),
            "connected layer's active model must be listed: {out}"
        );
        assert!(
            out.contains("winner: cli-overlay") && out.contains("gpt-4o"),
            "cli-overlay beats connected for this invocation: {out}"
        );
    }

    #[test]
    fn base_url_alias_reads_the_active_provider_slot() {
        let out = render_explain("base_url", &fixture_dump());
        assert!(
            out.contains("https://open.bigmodel.cn/v1"),
            "base_url must come from the active provider slot: {out}"
        );
        assert!(out.contains("winner: connected"), "{out}");
    }

    #[test]
    fn no_layer_defining_falls_back_to_runtime_default_note() {
        let dump = {
            let layers = vec![
                LayerSnapshot::builtin(),
                snap(
                    "user-global",
                    Some("/h/.shannon/config.toml"),
                    r#"{"debug": true}"#,
                ),
            ];
            let merged = fold_layers(&layers.iter().map(|l| l.config.clone()).collect::<Vec<_>>());
            build_dump(&layers, &merged)
        };
        let out = render_explain("temperature", &dump);
        assert!(out.contains("no config layer defines temperature"), "{out}");
        assert!(
            out.contains("SHANNON_TEMPERATURE"),
            "hint still shown: {out}"
        );
    }

    #[test]
    fn unknown_key_lists_every_known_key() {
        let out = render_explain("modle", &fixture_dump());
        assert!(out.contains("unknown config key: 'modle'"), "{out}");
        for known in [
            "max_tokens",
            "temperature",
            "provider_model",
            "model",
            "hooks",
        ] {
            assert!(
                out.contains(known),
                "known key '{known}' must be listed: {out}"
            );
        }
        assert!(
            out.contains("--dump-config"),
            "pointer to the JSON surface: {out}"
        );
    }

    #[test]
    fn secret_shaped_key_gets_the_credentials_answer() {
        let out = render_explain("anthropic_api_key", &fixture_dump());
        assert!(out.contains("decision A1"), "{out}");
        assert!(out.contains("credential store"), "{out}");
        assert!(out.contains("/connect"), "{out}");
        assert!(
            !out.contains("winner:"),
            "credential keys have no layer ladder: {out}"
        );
    }

    #[test]
    fn key_matching_is_case_insensitive_and_trimmed() {
        let out = render_explain("  MAX_TOKENS  ", &fixture_dump());
        assert!(out.starts_with("key: MAX_TOKENS"), "{out}");
        assert!(out.contains("winner: env-vars"), "{out}");
    }

    // ── desktop-only keys (P-N18) ───────────────────────────────────────

    #[test]
    fn desktop_only_keys_get_the_desktop_answer_with_path_and_hint() {
        for key in [
            "plan_tier",
            "act_tier",
            "effort_level",
            "approval_mode",
            "enabled_providers",
        ] {
            let out = render_explain(key, &fixture_dump());
            assert!(out.contains(&format!("key: {key}")), "{key}: {out}");
            assert!(
                out.contains("desktop-UI-only") && out.contains("桌面"),
                "{key} must be named desktop-UI-only: {out}"
            );
            assert!(
                out.contains("~/.shannon/desktop/config.json"),
                "{key} must name the desktop config path: {out}"
            );
            assert!(
                out.contains("the engine never reads it"),
                "{key} must state the engine ignores it: {out}"
            );
            assert!(
                out.contains("change it:"),
                "{key} must point at where to change it: {out}"
            );
            // Every key's hint names its own Settings surface (effort lives
            // in the composer's model menu, not a Settings pane).
            if key == "effort_level" {
                assert!(out.contains("composer"), "{key}: {out}");
            } else {
                assert!(out.contains("Settings"), "{key}: {out}");
            }
            // No layer ladder: the engine layers cannot define these keys.
            assert!(
                !out.contains("winner:") && !out.contains("defined by"),
                "{key} must not render a layer ladder: {out}"
            );
        }
    }

    #[test]
    fn desktop_key_answer_is_identical_without_a_configured_value() {
        // Unconfigured (no desktop config file readable): the static answer,
        // no "current desktop value" line.
        let plain = render_desktop_key("plan_tier", "plan_tier", None);
        assert!(
            !plain.contains("current desktop value"),
            "no value line when unconfigured: {plain}"
        );
        // Configured: same answer, plus the read-only value line (strings
        // render bare, per the display_value contract).
        let configured =
            render_desktop_key("plan_tier", "plan_tier", Some(&serde_json::json!("pro")));
        assert!(
            configured.contains("current desktop value: pro"),
            "{configured}"
        );
        for line in plain.lines() {
            if line.contains("current desktop value") {
                continue;
            }
            assert!(
                configured.contains(line),
                "configured answer keeps the static answer, differs only by the value line: {line}"
            );
        }
    }

    #[test]
    fn desktop_keys_are_case_insensitive_and_listed_as_known() {
        let out = render_explain("PLAN_TIER", &fixture_dump());
        assert!(out.contains("desktop-UI-only"), "{out}");
        // Unknown-key listing now includes the desktop keys.
        let unknown = render_explain("plan_tier_typo", &fixture_dump());
        for key in [
            "plan_tier",
            "act_tier",
            "effort_level",
            "approval_mode",
            "enabled_providers",
        ] {
            assert!(
                unknown.contains(key),
                "known keys list must include {key}: {unknown}"
            );
        }
    }
}
