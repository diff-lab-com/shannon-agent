//! review §P2-21 — application-level ACL coverage safety net.
//!
//! Once `desktop/build.rs` publishes the `__app-acl__` manifest,
//! `RuntimeAuthority::has_app_manifest()` is true and **every** invoke is
//! ACL-checked: a command without a grant is denied at runtime. That makes
//! an incomplete ACL a functional break, not just a hardening gap, so this
//! test is the compile-adjacent tripwire:
//!
//! 1. the command inventory is re-derived from `generate_handler![...]` in
//!    `src/main.rs` (text scan in `tests/common/mod.rs`);
//! 2. `acl/app-permissions.json` must grant **exactly** that inventory —
//!    a missing entry (new command without ACL) and a stale entry (removed
//!    command still granted) both fail, i.e. the manifest↔handler diff
//!    must be empty in both directions;
//! 3. every capability file must parse, reference only defined
//!    permissions/sets (app manifest or the plugin manifests `build.rs`
//!    actually defines), stay local-only, and only target known window
//!    patterns (`main`, `session-*`);
//! 4. the union of grants for `main` **and** for a `session-*` window must
//!    each cover the full inventory (both windows run the same SPA), while
//!    unknown window labels match no capability at all.
//!
//! Maintenance path when adding a command: append it to `generate_handler!`
//! in `main.rs`, add an `allow-<command>` permission to
//! `acl/app-permissions.json`, put it in the matching `app-*` set (create
//! one if needed), and the sets are already referenced by
//! `capabilities/app-commands.json`. This test tells you exactly that in
//! its failure message.

use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

#[allow(dead_code)]
mod common;

use common::{desktop_dir, handler_inventory};

/// Window label patterns the app is allowed to target in capability files:
/// the `main` window from tauri.conf.json and the `session-{uuid}` windows
/// created by `session_window_commands` (`SESSION_WINDOW_PREFIX = "session-"`).
const KNOWN_WINDOW_PATTERNS: [&str; 2] = ["main", "session-*"];

/// Plugin manifests `desktop/build.rs` actually defines. `build.rs` fails
/// the compile on anything else via `Resolved::resolve`; this mirror keeps
/// the test failure readable when the drift is in a capability file.
const DEFINED_PLUGIN_MANIFESTS: [&str; 3] = ["core:event", "core:window", "dialog"];

// ---------------------------------------------------------------------------
// File-format structs (mirror the tauri-utils ACL types we consume)
// ---------------------------------------------------------------------------

/// `acl/app-permissions.json` — tauri-utils `PermissionFile` shape
/// (`default` / `permission` / `set`), consumed by `build.rs` via
/// `Manifest::new`. `default` is deliberately `null`: there is no
/// `__app-acl__:default` permission that would grant everything at once.
#[derive(Deserialize)]
struct AppPermissions {
    #[serde(default)]
    permission: Vec<PermissionJson>,
    #[serde(default)]
    set: Vec<PermissionSetJson>,
}

#[derive(Deserialize)]
struct PermissionJson {
    identifier: String,
    #[serde(default)]
    commands: CommandsJson,
}

#[derive(Deserialize, Default)]
struct CommandsJson {
    #[serde(default)]
    allow: Vec<String>,
    #[serde(default)]
    deny: Vec<String>,
}

#[derive(Deserialize)]
struct PermissionSetJson {
    identifier: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    permissions: Vec<String>,
}

/// Flattened view for the assertions below.
struct AppManifest {
    /// permission identifier -> allowed command names
    permissions: BTreeMap<String, Vec<String>>,
    /// set identifier -> member identifiers (permissions or nested sets)
    sets: BTreeMap<String, Vec<String>>,
}

fn load_app_manifest() -> AppManifest {
    let path = desktop_dir().join("acl").join("app-permissions.json");
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    let file: AppPermissions = serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("failed to parse {}: {e}", path.display()));
    let mut manifest = AppManifest {
        permissions: BTreeMap::new(),
        sets: BTreeMap::new(),
    };
    for permission in file.permission {
        assert!(
            manifest
                .permissions
                .insert(permission.identifier.clone(), permission.commands.allow)
                .is_none(),
            "duplicate permission identifier `{}` in {}",
            permission.identifier,
            path.display()
        );
    }
    for set in file.set {
        assert!(
            manifest
                .sets
                .insert(set.identifier.clone(), set.permissions)
                .is_none(),
            "duplicate permission set identifier `{}` in {}",
            set.identifier,
            path.display()
        );
    }
    assert!(!manifest.permissions.is_empty(), "empty app ACL manifest");
    manifest
}

/// Capability files under `capabilities/` (single-object form, the only
/// form this package uses — `build.rs` also accepts lists, which these
/// assertions would need extending for).
#[derive(Deserialize)]
struct CapabilityJson {
    identifier: String,
    #[serde(default)]
    windows: Vec<String>,
    #[serde(default)]
    webviews: Vec<String>,
    /// `local` defaults to true in tauri-utils; only `false` would be a
    /// remote-only capability, which the app must never ship.
    #[serde(default)]
    local: Option<bool>,
    #[serde(default)]
    remote: Option<serde_json::Value>,
    #[serde(default)]
    permissions: Vec<serde_json::Value>,
}

fn load_capabilities() -> Vec<CapabilityJson> {
    let dir = desktop_dir().join("capabilities");
    let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", dir.display()))
        .collect::<Result<Vec<_>, _>>()
        .expect("capability directory entries")
        .into_iter()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    paths.sort();
    assert!(
        paths.len() >= 3,
        "expected at least the app-commands, session-windows and file-dialogs capabilities"
    );
    paths
        .iter()
        .map(|path| {
            let raw = std::fs::read_to_string(path)
                .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
            serde_json::from_str(&raw)
                .unwrap_or_else(|e| panic!("failed to parse {}: {e}", path.display()))
        })
        .collect()
}

/// Permission references can be plain strings or `{ identifier, .. }`
/// scope-extension objects; both are valid `PermissionEntry` values.
fn permission_identifier(entry: &serde_json::Value, capability: &str) -> String {
    match entry {
        serde_json::Value::String(id) => id.clone(),
        serde_json::Value::Object(map) => map
            .get("identifier")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| {
                panic!("capability {capability} has a permission object without `identifier`")
            }),
        other => panic!("capability {capability} has a malformed permission entry: {other}"),
    }
}

/// The manifest key a permission reference resolves against: `Some(plugin)`
/// for prefixed identifiers, `None` for app-manifest (`__app-acl__`) ones.
/// Mirrors `tauri_utils::acl::Identifier::get_prefix` (plus the `core:`
/// two-segment special case) closely enough for these fixed files.
fn plugin_prefix(id: &str) -> Option<String> {
    if let Some(rest) = id.strip_prefix("core:") {
        let (plugin, _) = rest.split_once(':')?;
        Some(format!("core:{plugin}"))
    } else {
        id.split_once(':').map(|(plugin, _)| plugin.to_string())
    }
}

fn glob_match(pattern: &str, label: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => label.starts_with(prefix),
        None => pattern == label,
    }
}

/// App commands granted to a concrete window label: the union over every
/// capability whose `windows` patterns match, resolving app-level
/// permission sets transitively. Plugin commands are out of scope here
/// (covered by the runtime tests in `session_window_acl.rs`).
fn app_commands_allowed_for_window(
    manifest: &AppManifest,
    capabilities: &[CapabilityJson],
    window: &str,
) -> BTreeSet<String> {
    let mut allowed = BTreeSet::new();
    for capability in capabilities {
        if !capability.windows.iter().any(|p| glob_match(p, window)) {
            continue;
        }
        for entry in &capability.permissions {
            let id = permission_identifier(entry, &capability.identifier);
            if plugin_prefix(&id).is_some() {
                continue;
            }
            resolve_app_reference(manifest, &id, &capability.identifier, &mut allowed);
        }
    }
    allowed
}

fn resolve_app_reference(
    manifest: &AppManifest,
    id: &str,
    capability: &str,
    allowed: &mut BTreeSet<String>,
) {
    resolve_app_reference_inner(manifest, id, capability, allowed, 0);
}

fn resolve_app_reference_inner(
    manifest: &AppManifest,
    id: &str,
    capability: &str,
    allowed: &mut BTreeSet<String>,
    depth: usize,
) {
    // a cycle in `set` membership must fail the test, not blow the stack
    assert!(
        depth < 16,
        "permission set nesting deeper than 16 (cycle?) at `{id}` referenced by {capability}"
    );
    if let Some(members) = manifest.sets.get(id) {
        for member in members {
            resolve_app_reference_inner(manifest, member, capability, allowed, depth + 1);
        }
    } else if let Some(commands) = manifest.permissions.get(id) {
        allowed.extend(commands.iter().cloned());
    } else {
        panic!(
            "capability {capability} references unknown app permission/set `{id}` \
             (acl/app-permissions.json defines neither)"
        );
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Visible sanity bound for the text parser (the design doc requires the
/// inventory count to be asserted so a parsing regression shows up as a
/// scale failure, not a silent empty diff).
#[test]
fn inventory_parses_at_expected_scale() {
    let inventory = handler_inventory();
    assert!(
        inventory.len() >= 200,
        "command inventory is {} (expected 200+): the generate_handler! parser regressed",
        inventory.len()
    );
}

/// The two-sided manifest↔generate_handler diff: must be empty.
#[test]
fn acl_manifest_matches_handler_inventory_exactly() {
    let inventory = handler_inventory();
    let manifest = load_app_manifest();

    let mut granted: BTreeMap<String, String> = BTreeMap::new();
    for (id, commands) in &manifest.permissions {
        let kebab = id.strip_prefix("allow-").unwrap_or_else(|| {
            panic!(
                "app permission `{id}` does not follow the autogenerated `allow-<command>` \
                 naming (kebab-case of the command)"
            )
        });
        assert!(
            !kebab.is_empty() && !kebab.starts_with('-') && !kebab.ends_with('-'),
            "app permission `{id}` is not a valid kebab-case identifier"
        );
        assert!(
            commands.len() == 1,
            "app permission `{id}` must allow exactly one command, found {commands:?}"
        );
        assert!(
            commands[0] == kebab.replace('-', "_"),
            "app permission `{id}` must allow exactly `{}` (autogenerated naming), found `{:?}`",
            kebab.replace('-', "_"),
            commands
        );
        assert!(
            granted.insert(commands[0].clone(), id.clone()).is_none(),
            "command `{}` granted by two permissions",
            commands[0]
        );
    }

    let granted_commands: BTreeSet<String> = granted.keys().cloned().collect();
    let missing: Vec<String> = inventory.difference(&granted_commands).cloned().collect();
    assert!(
        missing.is_empty(),
        "commands registered in generate_handler! but missing from acl/app-permissions.json \
         (add an `allow-<kebab>` permission and put it in the matching `app-*` set): {missing:?}"
    );

    let stale: Vec<String> = granted_commands
        .iter()
        .filter(|c| !inventory.contains(*c))
        .cloned()
        .collect();
    assert!(
        stale.is_empty(),
        "acl/app-permissions.json grants commands no longer registered in generate_handler! \
         (remove the permissions and their set entries): {stale:?}"
    );
}

#[test]
fn permission_sets_reference_only_defined_members_and_are_used() {
    let manifest = load_app_manifest();
    let capabilities = load_capabilities();

    let mut referenced_sets = BTreeSet::new();
    for capability in &capabilities {
        for entry in &capability.permissions {
            let id = permission_identifier(entry, &capability.identifier);
            if plugin_prefix(&id).is_none() {
                if let Some(members) = manifest.sets.get(&id) {
                    referenced_sets.insert(id.clone());
                    for member in members {
                        // every set member must resolve inside the manifest
                        resolve_app_reference(
                            &manifest,
                            member,
                            &capability.identifier,
                            &mut BTreeSet::new(),
                        );
                    }
                }
            }
        }
    }

    let orphans: Vec<String> = manifest
        .sets
        .keys()
        .filter(|s| !referenced_sets.contains(*s))
        .cloned()
        .collect();
    assert!(
        orphans.is_empty(),
        "app permission sets defined but never granted by any capability: {orphans:?}"
    );
}

#[test]
fn capabilities_are_local_only_and_target_known_windows() {
    for capability in load_capabilities() {
        assert!(
            capability.remote.is_none(),
            "capability `{}` grants a `remote` origin — the app ships no remote-content \
             windows, remote grants must never be added casually",
            capability.identifier
        );
        assert!(
            capability.local != Some(false),
            "capability `{}` is remote-only (local: false)",
            capability.identifier
        );
        assert!(
            capability.webviews.is_empty(),
            "capability `{}` targets specific webviews; the app scopes by window label only",
            capability.identifier
        );
        for pattern in &capability.windows {
            assert!(
                KNOWN_WINDOW_PATTERNS.contains(&pattern.as_str()),
                "capability `{}` targets unknown window pattern `{pattern}`; known patterns \
                 are {KNOWN_WINDOW_PATTERNS:?}",
                capability.identifier
            );
        }
        assert!(
            !capability.windows.is_empty(),
            "capability `{}` targets no windows (would resolve to nothing)",
            capability.identifier
        );
    }
}

#[test]
fn capabilities_reference_only_defined_permissions() {
    let manifest = load_app_manifest();
    for capability in load_capabilities() {
        for entry in &capability.permissions {
            let id = permission_identifier(entry, &capability.identifier);
            match plugin_prefix(&id) {
                Some(plugin) => assert!(
                    DEFINED_PLUGIN_MANIFESTS.contains(&plugin.as_str()),
                    "capability `{}` references `{id}` but desktop/build.rs defines no \
                     `{plugin}` manifest (add one there and to DEFINED_PLUGIN_MANIFESTS here)",
                    capability.identifier
                ),
                None => assert!(
                    manifest.permissions.contains_key(&id) || manifest.sets.contains_key(&id),
                    "capability `{}` references unknown app permission `{id}`",
                    capability.identifier
                ),
            }
        }
    }
}

/// Every command must be granted to BOTH window groups: `main` and a
/// `session-*` window (the same SPA bundle runs in both), and no unknown
/// window label may match anything.
#[test]
fn every_command_is_allowed_on_main_and_session_windows() {
    let inventory = handler_inventory();
    let manifest = load_app_manifest();
    let capabilities = load_capabilities();

    let session_window = "session-00000000-0000-0000-0000-000000000000";
    for window in ["main", session_window] {
        let allowed = app_commands_allowed_for_window(&manifest, &capabilities, window);
        let missing: Vec<String> = inventory.difference(&allowed).cloned().collect();
        assert!(
            missing.is_empty(),
            "window group `{window}` is not granted {} app command(s): {missing:?} — \
             extend capabilities/app-commands.json (via acl/app-permissions.json sets)",
            missing.len()
        );
    }

    for impostor in ["untrusted-window", "sessionx-123", "session"] {
        let allowed = app_commands_allowed_for_window(&manifest, &capabilities, impostor);
        assert!(
            allowed.is_empty(),
            "window label `{impostor}` unexpectedly matched a capability and got {} \
             app command(s)",
            allowed.len()
        );
    }
}
