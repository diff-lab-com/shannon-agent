//! Integration tests for custom permission profiles.
//!
//! Tests the full pipeline: load from dirs → resolve by name → apply to PermissionManager.

use shannon_engine::custom_profiles::{CustomProfileDef, CustomProfileRegistry};
use shannon_engine::permission_profile::PermissionProfile;
use shannon_engine::permissions::{ApprovalMode, PermissionManager};
use std::fs;

fn write_profile(dir: &std::path::Path, name: &str, content: &str) {
    fs::write(dir.join(format!("{name}.toml")), content).unwrap();
}

// ── Registry loading ────────────────────────────────────────────────────────

#[test]
fn load_multiple_profiles_from_dir() {
    let dir = tempfile::tempdir().unwrap();
    write_profile(
        dir.path(),
        "trusted",
        r#"name = "trusted"
description = "Full access"
auto_approve = ["Read", "Glob", "Grep", "Bash", "Edit", "Write"]
"#,
    );
    write_profile(
        dir.path(),
        "readonly",
        r#"name = "readonly"
description = "Read only"
auto_approve = ["Read", "Glob", "Grep"]
deny = ["Bash", "Write", "Edit"]
"#,
    );

    let mut registry = CustomProfileRegistry::new();
    registry.load_from_dir(dir.path());

    assert_eq!(registry.all().len(), 2);
    assert!(registry.get("trusted").is_some());
    assert!(registry.get("readonly").is_some());
}

#[test]
fn local_dir_overrides_global() {
    let global = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();

    write_profile(
        global.path(),
        "shared",
        r#"name = "shared"
description = "Global version"
auto_approve = ["Read"]
"#,
    );
    write_profile(
        local.path(),
        "shared",
        r#"name = "shared"
description = "Local version"
auto_approve = ["Read", "Write"]
"#,
    );

    let mut registry = CustomProfileRegistry::new();
    registry.load_from_dir(global.path());
    registry.load_from_dir(local.path());

    let def = registry.get("shared").unwrap();
    assert_eq!(def.description, "Local version");
    assert!(def.auto_approve.contains(&"Write".to_string()));
}

#[test]
fn bad_toml_does_not_block_valid_profiles() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("bad.toml"), "not valid toml [[[[").unwrap();
    write_profile(
        dir.path(),
        "good",
        r#"name = "good"
description = "Valid profile"
"#,
    );

    let mut registry = CustomProfileRegistry::new();
    registry.load_from_dir(dir.path());

    assert!(registry.get("good").is_some());
    assert!(registry.get("bad").is_none());
}

// ── PermissionManager integration ───────────────────────────────────────────

#[test]
fn apply_trusted_custom_profile() {
    let def = CustomProfileDef {
        name: "trusted".to_string(),
        description: "Full access".to_string(),
        auto_approve: vec![
            "Read".to_string(),
            "Glob".to_string(),
            "Grep".to_string(),
            "Bash".to_string(),
            "Edit".to_string(),
            "Write".to_string(),
        ],
        confirm: vec![],
        deny: vec![],
    };

    let mut pm = PermissionManager::new();
    pm.apply_custom_profile_def(&def);

    assert_eq!(pm.approval_mode(), ApprovalMode::AutoEdit);
    let profile = pm.active_profile().unwrap();
    assert_eq!(profile.to_string(), "custom:trusted");
}

#[test]
fn apply_readonly_custom_profile() {
    let def = CustomProfileDef {
        name: "readonly".to_string(),
        description: "Read only".to_string(),
        auto_approve: vec!["Read".to_string(), "Glob".to_string(), "Grep".to_string()],
        confirm: vec!["Edit".to_string()],
        deny: vec!["Bash".to_string(), "Write".to_string()],
    };

    let mut pm = PermissionManager::new();
    pm.apply_custom_profile_def(&def);

    assert_eq!(pm.approval_mode(), ApprovalMode::Suggest);
    assert!(pm.is_tool_destructive("Bash"));
    assert!(pm.is_tool_destructive("Write"));
    assert!(!pm.is_tool_destructive("Read"));
}

#[test]
fn apply_custom_profile_then_builtin() {
    let custom = CustomProfileDef {
        name: "custom1".to_string(),
        description: String::new(),
        auto_approve: vec!["Read".to_string(), "Write".to_string(), "Bash".to_string()],
        confirm: vec![],
        deny: vec![],
    };

    let mut pm = PermissionManager::new();
    pm.apply_custom_profile_def(&custom);
    assert_eq!(pm.approval_mode(), ApprovalMode::AutoEdit);

    // Switch to built-in strict — should override
    pm.apply_profile(PermissionProfile::Strict);
    assert_eq!(pm.active_profile().unwrap().to_string(), "strict");
}

// ── End-to-end: registry → profile → permission manager ────────────────────

#[test]
fn full_pipeline_load_and_apply() {
    let dir = tempfile::tempdir().unwrap();
    write_profile(
        dir.path(),
        "dev-trusted",
        r#"name = "dev-trusted"
description = "Dev environment with full access"
auto_approve = ["Read", "Glob", "Grep", "Bash", "Edit", "Write", "LS"]
confirm = []
deny = []
"#,
    );
    write_profile(
        dir.path(),
        "audit",
        r#"name = "audit"
description = "Audit mode - read only"
auto_approve = ["Read", "Glob", "Grep"]
deny = ["Bash", "Write", "Edit", "MultiEdit"]
"#,
    );

    let mut registry = CustomProfileRegistry::new();
    registry.load_from_dir(dir.path());

    // Apply dev-trusted
    let dev_profile = registry
        .get("dev-trusted")
        .expect("dev-trusted should exist");
    let mut pm = PermissionManager::new();
    pm.apply_custom_profile_def(dev_profile);
    assert_eq!(pm.approval_mode(), ApprovalMode::AutoEdit);

    // Apply audit
    let audit_profile = registry.get("audit").expect("audit should exist");
    pm.apply_custom_profile_def(audit_profile);
    assert_eq!(pm.approval_mode(), ApprovalMode::Suggest);
    assert!(pm.is_tool_destructive("Bash"));
    assert!(pm.is_tool_destructive("MultiEdit"));
}

#[test]
fn summary_reports_loaded_profiles() {
    let dir = tempfile::tempdir().unwrap();
    write_profile(
        dir.path(),
        "ci",
        r#"name = "ci"
description = "CI pipeline profile"
auto_approve = ["Read", "Bash"]
deny = ["Write"]
"#,
    );

    let mut registry = CustomProfileRegistry::new();
    registry.load_from_dir(dir.path());

    let summary = registry.summary();
    assert!(summary.contains("1 custom profile"));
    assert!(summary.contains("ci"));
    assert!(summary.contains("CI pipeline"));
}
