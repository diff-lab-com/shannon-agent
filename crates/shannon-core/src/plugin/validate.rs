//! Install-time manifest validation (§4.10 W3-2).
//!
//! Two jobs, one entry point ([`validate_for_install`]):
//!
//! 1. **Schema** — every structurally required field for the declared plugin
//!    kind must resolve (`kind()`), transports must carry their command/url,
//!    paths stay inside the plugin directory, hook names resolve against
//!    [`shannon_engine::hooks::HookEventType`], and MCP references are
//!    well-formed with unique handles.
//! 2. **Permission completeness** — a manifest's shape *implies* a set of
//!    Shannon-side capability faces (spawning an stdio server implies
//!    `execute_commands`, a remote transport implies `network`, tool routing
//!    implies `mcp_tools`, prompt-driven extensions imply `read_files` +
//!    `llm_api`). Under-enumerated declarations would surface only as
//!    runtime refusals; this module moves that failure to install time:
//!
//!    - **v2 manifests**: missing implied faces are a hard error.
//!    - **v1 / claude dialects**: legacy entries keep installing, but each
//!      gap becomes a returned warning the caller logs (`declaration =
//!      allow-set`, see `PERMISSIONS.md`).
//!
//! Loading an already-installed plugin deliberately does **not** re-run the
//! completeness half: §4.10 keeps upgrade paths non-breaking for plugins
//! that predate enforcement.

use super::{
    PluginError, PluginKind, PluginManifest, PluginPermission,
    manifest::{ManifestVersion, McpServerRef, parse_semver},
};
use std::collections::BTreeSet;

/// Capability faces implied by the manifest's own shape.
fn implied_faces(manifest: &PluginManifest) -> Result<BTreeSet<PluginPermission>, String> {
    let mut faces = BTreeSet::new();
    match manifest.kind()? {
        PluginKind::Tool { transport } => {
            if transport.is_stdio() {
                faces.insert(PluginPermission::ExecuteCommands);
            } else {
                faces.insert(PluginPermission::Network);
            }
            // Every tool plugin's tools route as `mcp__<name>__*`.
            faces.insert(PluginPermission::McpTools);
        }
        PluginKind::Command { .. } | PluginKind::Skill { .. } => {
            // Host reads the entry/template file and drives model turns.
            faces.insert(PluginPermission::ReadFiles);
            faces.insert(PluginPermission::LlmApi);
        }
    }

    // Bundled MCP references hit the same gates under the plugin's policy.
    for r in &manifest.mcp {
        match stdio_ref(r) {
            true => {
                faces.insert(PluginPermission::ExecuteCommands);
            }
            false => {
                faces.insert(PluginPermission::Network);
            }
        }
        faces.insert(PluginPermission::McpTools);
    }
    Ok(faces)
}

/// Is this MCP reference a local stdio server?
fn stdio_ref(r: &McpServerRef) -> bool {
    !matches!(r.transport_type.as_str(), "sse" | "http")
}

/// Relative path stays inside the plugin directory (no absolute / traversal).
fn safe_relative_path(kind_of_path: &str, value: &str) -> Result<(), PluginError> {
    let p = std::path::Path::new(value);
    let bad = matches!(
        p.components().next(),
        Some(std::path::Component::RootDir)
            | Some(std::path::Component::ParentDir)
            | Some(std::path::Component::Prefix(_))
    ) || p.components().any(|c| c == std::path::Component::ParentDir);
    if bad {
        return Err(PluginError::InvalidManifest(format!(
            "{kind_of_path} '{value}' must be relative to the plugin directory"
        )));
    }
    Ok(())
}

/// Validate a parsed manifest for installation.
///
/// `Ok(warnings)` means the plugin may install; the warnings are human-
/// readable completeness notes the caller should log verbatim (v1/claude
/// gap reporting). `Err` refuses installation.
pub fn validate_for_install(manifest: &PluginManifest) -> Result<Vec<String>, PluginError> {
    if manifest.name.trim().is_empty() {
        return Err(PluginError::InvalidManifest(
            "manifest 'name' must not be empty".into(),
        ));
    }
    if manifest.description.trim().is_empty() {
        return Err(PluginError::InvalidManifest(format!(
            "plugin '{}': 'description' must not be empty",
            manifest.name
        )));
    }
    if parse_semver(&manifest.version).is_none() {
        return Err(PluginError::InvalidManifest(format!(
            "plugin '{}': version '{}' is not a semver triple",
            manifest.name, manifest.version
        )));
    }
    safe_relative_path("entry", &manifest.entry)?;

    // kind() resolution doubles as per-kind schema validation (transport
    // presence, trigger/template/command_name presence, unknown types,
    // reserved wasm slot).
    let _kind = manifest
        .kind()
        .map_err(|e| PluginError::InvalidManifest(format!("plugin '{}': {e}", manifest.name)))?;

    // Hook subscriptions: reserved schema slot, but validate now so typo'd
    // event names fail at install time rather than when the protocol lands.
    for h in &manifest.hooks {
        if shannon_engine::hooks::HookEventType::from_str_lossy(&h.event).is_none() {
            return Err(PluginError::InvalidManifest(format!(
                "plugin '{}': unknown hook event '{}' (see HookEventType)",
                manifest.name, h.event
            )));
        }
        if h.handler.trim().is_empty() {
            return Err(PluginError::InvalidManifest(format!(
                "plugin '{}': hook '{}' has an empty handler",
                manifest.name, h.event
            )));
        }
        safe_relative_path(&format!("hook '{}' handler", h.event), &h.handler)?;
    }

    // Bundled MCP references: known transports, per-type fields, unique names.
    let mut seen_names = BTreeSet::new();
    for r in &manifest.mcp {
        if !seen_names.insert(r.name.clone()) {
            return Err(PluginError::InvalidManifest(format!(
                "plugin '{}': duplicate mcp server name '{}'",
                manifest.name, r.name
            )));
        }
        match r.transport_type.as_str() {
            "stdio" if r.command.is_some() => {}
            "stdio" => {
                return Err(PluginError::InvalidManifest(format!(
                    "plugin '{}': mcp server '{}' uses stdio without a command",
                    manifest.name, r.name
                )));
            }
            "sse" | "http" if r.url.is_some() => {}
            "sse" | "http" => {
                return Err(PluginError::InvalidManifest(format!(
                    "plugin '{}': mcp server '{}' uses {} without a url",
                    manifest.name, r.name, r.transport_type
                )));
            }
            other => {
                return Err(PluginError::InvalidManifest(format!(
                    "plugin '{}': mcp server '{}' has unsupported transport '{other}'",
                    manifest.name, r.name
                )));
            }
        }
    }

    // Permission completeness: shape-implied faces minus the declaration.
    let implied = implied_faces(manifest)
        .map_err(|e| PluginError::InvalidManifest(format!("plugin '{}': {e}", manifest.name)))?;
    let declared: BTreeSet<_> = manifest.permissions.iter().copied().collect();
    let missing: Vec<&'static str> = implied
        .iter()
        .filter(|f| !declared.contains(*f))
        .map(|f| f.wire_name())
        .collect();

    let mut warnings = Vec::new();
    if !missing.is_empty() {
        match manifest.schema_version() {
            ManifestVersion::V2 => {
                return Err(PluginError::InvalidManifest(format!(
                    "v2 permission completeness: plugin '{}' declares [{}] but its shape implies {} — declare {} or drop manifest_version = \"2\"",
                    manifest.name,
                    manifest
                        .permissions
                        .iter()
                        .map(|p| p.wire_name())
                        .collect::<Vec<_>>()
                        .join(", "),
                    missing.join(", "),
                    missing.join(", "),
                )));
            }
            ManifestVersion::V1 => {
                warnings.push(format!(
                    "plugin '{}' declares permissions [{}] but omits implied face(s) {} — runtime gates will refuse those actions until they are added (declaration = allow-set)",
                    manifest.name,
                    manifest
                        .permissions
                        .iter()
                        .map(|p| p.wire_name())
                        .collect::<Vec<_>>()
                        .join(", "),
                    missing.join(", "),
                ));
            }
        }
    }

    // Declared compat window vs the running build: loud note, not refusal.
    let current = env!("CARGO_PKG_VERSION");
    if !manifest.is_within_compat(current) {
        warnings.push(format!(
            "plugin '{}' declares a compat window outside this Shannon ({current}); verify before relying on it",
            manifest.name
        ));
    }

    Ok(warnings)
}

/// Log install-time warnings the way every caller should.
pub fn warn_about(warnings: &[String]) {
    for w in warnings {
        tracing::warn!(target: "plugin/install", "{w}");
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const V2_TOOL_OK: &str = r#"
manifest_version = "2"
name = "fs-tool"
version = "1.2.3"
description = "complete v2 tool"
type = "tool"
entry = "bin/server.js"

[transport]
type = "stdio"
command = "node"
args = ["bin/server.js"]
permissions_note_ignored = true

[[mcp]]
name = "sidecar"
type = "stdio"
command = "npx"
args = ["-y", "thing"]
"#;

    fn parse(toml: &str) -> PluginManifest {
        PluginManifest::from_toml(toml).expect("test manifest parses")
    }

    #[test]
    fn complete_v2_tool_installs_cleanly() {
        let toml = V2_TOOL_OK.replace(
            "entry = \"bin/server.js\"",
            "entry = \"bin/server.js\"\npermissions = [\"read_files\", \"write_files\", \"execute_commands\", \"network\", \"mcp_tools\"]",
        );
        let manifest = parse(&toml);
        let warnings = validate_for_install(&manifest).expect("complete v2 installs");
        assert!(
            warnings.is_empty(),
            "no warnings expected, got {warnings:?}"
        );
    }

    #[test]
    fn v2_missing_implied_face_is_a_hard_error_naming_the_face() {
        let toml = V2_TOOL_OK.replace(
            "entry = \"bin/server.js\"",
            "entry = \"bin/server.js\"\npermissions = [\"network\", \"execute_commands\"]",
        );
        let manifest = parse(&toml);
        let err = validate_for_install(&manifest).unwrap_err().to_string();
        assert!(err.contains("v2 permission completeness"), "{err}");
        assert!(err.contains("mcp_tools"), "{err}");
    }

    #[test]
    fn v1_with_gaps_installs_but_warns_with_wire_names() {
        let toml = r#"
name = "legacy-skill"
version = "0.1.0"
description = "predates enforcement"
type = "skill"
entry = "template.md"
trigger = "/legacy"
template = "hi"
"#;
        let manifest = parse(toml);
        assert_eq!(manifest.schema_version(), ManifestVersion::V1);
        let warnings = validate_for_install(&manifest).expect("v1 gaps only warn");
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("read_files") && warnings[0].contains("llm_api"));
    }

    #[test]
    fn claude_dialect_treated_as_v1_for_completeness() {
        let json = r#"{"name":"cc-cmd","version":"1.0.0","description":"d",
            "type":"command","entry":"cmd.md","command_name":"say"}"#;
        let manifest = PluginManifest::from_json(json).expect("claude parses");
        let warnings = validate_for_install(&manifest).expect("warning-only");
        assert!(warnings[0].contains("read_files"));
    }

    #[test]
    fn structural_failures_naming_context() {
        let bad_version = r#"
name = "x"
version = "one"
description = "d"
type = "skill"
entry = "t.md"
trigger = "/t"
template = "t"
"#;
        let err = validate_for_install(&parse(bad_version))
            .unwrap_err()
            .to_string();
        assert!(err.contains("not a semver triple"), "{err}");

        let traversal = r#"
name = "y"
version = "1.0.0"
description = "d"
type = "skill"
entry = "../../etc/passwd"
trigger = "/t"
template = "t"
"#;
        let err = validate_for_install(&parse(traversal))
            .unwrap_err()
            .to_string();
        assert!(err.contains("must be relative"), "{err}");

        let empty_desc = r#"
name = "z"
version = "1.0.0"
description = ""
type = "command"
entry = "t.md"
command_name = "z"
"#;
        let err = validate_for_install(&parse(empty_desc))
            .unwrap_err()
            .to_string();
        assert!(err.contains("'description' must not be empty"), "{err}");
    }

    #[test]
    fn reserved_wasm_type_reports_the_slot_not_unknown_type() {
        let manifest = PluginManifest::from_toml(
            r#"
name = "wannabe-wasm"
version = "1.0.0"
description = "reserved type"
type = "wasm"
entry = "guest.wasm"
"#,
        )
        .expect("parses");
        let err = validate_for_install(&manifest).unwrap_err().to_string();
        assert!(err.contains("reserved schema slot"), "{err}");
        assert!(!err.contains("unknown plugin type"), "{err}");
    }

    #[test]
    fn hook_validation_catches_typo_and_traversal_handler() {
        let typo = r#"
name = "hooked"
version = "1.0.0"
description = "hooks"
type = "skill"
entry = "t.md"
trigger = "/h"
template = "t"
manifest_version = "2"
permissions = ["read_files", "llm_api"]

[[hooks]]
event = "PostToolUsse"
handler = "hooks/lint.sh"
"#;
        let err = validate_for_install(&parse(typo)).unwrap_err().to_string();
        assert!(err.contains("unknown hook event"), "{err}");

        let escape = r#"
name = "hooked2"
version = "1.0.0"
description = "hooks"
type = "skill"
entry = "t.md"
trigger = "/h"
template = "t"
manifest_version = "2"
permissions = ["read_files", "llm_api"]

[[hooks]]
event = "PostToolUse"
handler = "../evil.sh"
"#;
        let err = validate_for_install(&parse(escape))
            .unwrap_err()
            .to_string();
        assert!(err.contains("must be relative"), "{err}");
    }

    #[test]
    fn mcp_refs_unique_and_transport_complete() {
        let dup = r#"
name = "dupe"
version = "1.0.0"
description = "d"
type = "tool"
entry = "s.js"
permissions = ["execute_commands", "network", "mcp_tools"]
[transport]
type = "stdio"
command = "node"

[[mcp]]
name = "same"
command = "a"

[[mcp]]
name = "same"
url = "http://x/sse"
type = "sse"
"#;
        let err = validate_for_install(&parse(dup)).unwrap_err().to_string();
        assert!(err.contains("duplicate mcp server name"), "{err}");

        let naked_stdio = r#"
name = "naked"
version = "1.0.0"
description = "d"
type = "tool"
entry = "s.js"
permissions = ["execute_commands", "network", "mcp_tools"]
[transport]
type = "stdio"
command = "node"

[[mcp]]
name = "half"
type = "stdio"
"#;
        let err = validate_for_install(&parse(naked_stdio))
            .unwrap_err()
            .to_string();
        assert!(err.contains("stdio without a command"), "{err}");
    }

    #[test]
    fn out_of_window_compat_warns_but_installs() {
        let toml = r#"
name = "fussy"
version = "1.0.0"
description = "d"
type = "skill"
entry = "t.md"
trigger = "/f"
template = "t"
manifest_version = "2"
permissions = ["read_files", "llm_api"]

[compat]
min = "99.0.0"
max = "100.0.0"
"#;
        let manifest = parse(toml);
        let warnings = validate_for_install(&manifest).expect("warn-not-refuse");
        assert!(
            warnings.iter().any(|w| w.contains("compat window outside")),
            "{warnings:?}"
        );
    }

    #[test]
    fn semver_reader_basics() {
        assert_eq!(parse_semver("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_semver("v0.10.0-beta.1+meta"), Some((0, 10, 0)));
        assert_eq!(parse_semver("one"), None);
        assert_eq!(parse_semver("1.2"), None);
    }

    #[test]
    fn compat_range_bounds_are_min_inclusive_max_exclusive() {
        let range = crate::plugin::manifest::CompatRange {
            min: Some("0.10.0".into()),
            max: Some("0.12.0".into()),
        };
        assert!(range.contains("0.10.0"));
        assert!(range.contains("0.11.9"));
        assert!(!range.contains("0.12.0"));
        assert!(!range.contains("0.9.9"));
    }
}
