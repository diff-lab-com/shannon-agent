//! Integration tests for the triggered routines and custom profiles systems.
//!
//! Tests cross-module behavior: TOML loading, event matching, execution,
//! and the interaction between routines, profiles, and the hook system.

use shannon_core::HookEventType;
use shannon_core::TriggeredRoutineRegistry;
use shannon_core::custom_profiles::{CustomProfileDef, CustomProfileRegistry};

// ============================================================================
// Triggered Routines Integration Tests
// ============================================================================

mod triggered_routines_integration {
    use super::*;

    #[test]
    fn full_workflow_load_match_execute() {
        // Create a temp TOML file
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("routines.toml");
        std::fs::write(
            &path,
            r#"
[[routine]]
name = "post-edit-lint"
trigger = "PostToolUse"
matcher = "Edit|Write"
command = "echo lint-ran"
description = "Run linter after edits"

[[routine]]
name = "post-edit-format"
trigger = "PostToolUse"
matcher = "Edit"
command = "echo format-ran"

[[routine]]
name = "file-change-check"
trigger = "FileChanged"
pattern = "*.rs"
command = "cargo check"
"#,
        )
        .unwrap();

        // Load
        let mut registry = TriggeredRoutineRegistry::new();
        registry.load_from_file(&path);
        assert_eq!(registry.all().len(), 3);

        // Match PostToolUse + Edit
        let matching = registry.matching_routines(&HookEventType::PostToolUse, "Edit");
        assert_eq!(matching.len(), 2); // lint + format

        // Match PostToolUse + Write
        let matching = registry.matching_routines(&HookEventType::PostToolUse, "Write");
        assert_eq!(matching.len(), 1); // lint only
        assert_eq!(matching[0].name, "post-edit-lint");

        // Match PostToolUse + Read → no match
        let matching = registry.matching_routines(&HookEventType::PostToolUse, "Read");
        assert!(matching.is_empty());

        // Match FileChanged with path filter
        let matching = registry.matching_routines_with_path(
            &HookEventType::FileChanged,
            "",
            Some("src/main.rs"),
        );
        assert_eq!(matching.len(), 1);

        let matching = registry.matching_routines_with_path(
            &HookEventType::FileChanged,
            "",
            Some("style.css"),
        );
        assert!(matching.is_empty());
    }

    #[test]
    fn merge_multiple_files() {
        let dir1 = tempfile::tempdir().unwrap();
        let dir2 = tempfile::tempdir().unwrap();

        std::fs::write(
            dir1.path().join("routines.toml"),
            r#"
[[routine]]
name = "global-lint"
trigger = "PostToolUse"
command = "echo global"
"#,
        )
        .unwrap();

        std::fs::write(
            dir2.path().join("routines.toml"),
            r#"
[[routine]]
name = "global-lint"
trigger = "PostToolUse"
command = "echo local"
"#,
        )
        .unwrap();

        let mut registry = TriggeredRoutineRegistry::new();
        registry.load_from_file(&dir1.path().join("routines.toml"));
        registry.load_from_file(&dir2.path().join("routines.toml"));

        // Local should override global
        assert_eq!(registry.get("global-lint").unwrap().command, "echo local");
    }

    #[tokio::test]
    async fn execute_matching_e2e() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("routines.toml");
        std::fs::write(
            &path,
            r#"
[[routine]]
name = "hello"
trigger = "Stop"
command = "echo hello-from-routine"
"#,
        )
        .unwrap();

        let mut registry = TriggeredRoutineRegistry::new();
        registry.load_from_file(&path);

        let results = registry
            .execute_matching(&HookEventType::Stop, "", None)
            .await;
        assert_eq!(results.len(), 1);
        assert!(results[0].success());
        assert!(results[0].stdout.contains("hello-from-routine"));
    }

    #[test]
    fn all_hook_event_types_as_triggers() {
        let event_types = [
            "PreToolUse",
            "PostToolUse",
            "SessionStart",
            "SessionEnd",
            "Notification",
            "UserPromptSubmit",
            "PreCompact",
            "Stop",
            "FileChanged",
            "CwdChanged",
            "PermissionDenied",
            "ConfigChange",
        ];

        for et in &event_types {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("routines.toml");
            let toml = format!(
                r#"
[[routine]]
name = "test-{et}"
trigger = "{et}"
command = "echo {et}"
"#
            );
            std::fs::write(&path, &toml).unwrap();

            let mut registry = TriggeredRoutineRegistry::new();
            registry.load_from_file(&path);

            let parsed = HookEventType::from_str_lossy(et);
            assert!(parsed.is_some(), "Failed to parse event type: {et}");

            let matching = registry.matching_routines(&parsed.unwrap(), "");
            assert_eq!(matching.len(), 1, "No match for event type: {et}");
        }
    }
}

// ============================================================================
// Custom Profiles Integration Tests
// ============================================================================

mod custom_profiles_integration {
    use super::*;

    #[test]
    fn full_workflow_load_query() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("trusted.toml"),
            r#"
name = "trusted"
description = "Full access for trusted projects"
auto_approve = ["Read", "Glob", "Grep", "Bash", "Edit", "Write"]
"#,
        )
        .unwrap();

        std::fs::write(
            dir.path().join("strict.toml"),
            r#"
name = "strict"
description = "Read-only access"
auto_approve = ["Read", "Glob", "Grep"]
deny = ["Bash", "Write"]
"#,
        )
        .unwrap();

        let mut registry = CustomProfileRegistry::new();
        registry.load_from_dir(dir.path());

        assert_eq!(registry.all().len(), 2);

        let trusted = registry.get("trusted").unwrap();
        assert_eq!(trusted.auto_approve.len(), 6);
        assert!(trusted.confirm.is_empty());
        assert!(trusted.deny.is_empty());

        let strict = registry.get("strict").unwrap();
        assert_eq!(strict.auto_approve.len(), 3);
        assert!(strict.deny.contains(&"Bash".to_string()));
        assert!(strict.deny.contains(&"Write".to_string()));
    }

    #[test]
    fn profile_override_chain() {
        let global = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();

        // Global profile
        std::fs::write(
            global.path().join("team.toml"),
            r#"
name = "team"
description = "Team default"
auto_approve = ["Read", "Glob"]
"#,
        )
        .unwrap();

        // Project-local override with more permissions
        std::fs::write(
            project.path().join("team.toml"),
            r#"
name = "team"
description = "Team local override"
auto_approve = ["Read", "Glob", "Grep", "Bash"]
confirm = ["Edit", "Write"]
"#,
        )
        .unwrap();

        let mut registry = CustomProfileRegistry::new();
        registry.load_from_dir(global.path());
        registry.load_from_dir(project.path());

        let profile = registry.get("team").unwrap();
        assert_eq!(profile.description, "Team local override");
        assert_eq!(profile.auto_approve.len(), 4);
        assert!(profile.auto_approve.contains(&"Bash".to_string()));
        assert_eq!(profile.confirm.len(), 2);
    }

    #[test]
    fn summary_with_multiple_profiles() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("a.toml"),
            r#"name = "alpha"
description = "Alpha profile"
auto_approve = ["Read"]
"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("b.toml"),
            r#"name = "beta"
description = "Beta profile"
deny = ["Bash"]
"#,
        )
        .unwrap();

        let mut registry = CustomProfileRegistry::new();
        registry.load_from_dir(dir.path());

        let summary = registry.summary();
        assert!(summary.contains("2 custom profile"));
        assert!(summary.contains("alpha"));
        assert!(summary.contains("beta"));
    }

    #[test]
    fn toml_serialization_roundtrip() {
        let def = CustomProfileDef {
            name: "roundtrip".to_string(),
            description: "Test roundtrip".to_string(),
            auto_approve: vec!["Read".to_string(), "Glob".to_string()],
            confirm: vec!["Edit".to_string()],
            deny: vec!["Bash".to_string()],
        };
        let toml_str = toml::to_string_pretty(&def).unwrap();
        let back: CustomProfileDef = toml::from_str(&toml_str).unwrap();
        assert_eq!(back.name, def.name);
        assert_eq!(back.auto_approve, def.auto_approve);
        assert_eq!(back.confirm, def.confirm);
        assert_eq!(back.deny, def.deny);
    }
}

// ============================================================================
// Scheduled Routines Integration Tests
// ============================================================================

mod scheduled_routines_integration {
    use shannon_core::scheduled_routines::{RoutineManager, ScheduledRoutine};

    #[test]
    fn save_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("routines.json");

        let mut mgr = RoutineManager::new();
        mgr.add(ScheduledRoutine::new(
            "daily-standup".into(),
            "Summarize yesterday's work".into(),
            86400,
        ));
        mgr.add(ScheduledRoutine::new(
            "hourly-check".into(),
            "Check for stale branches".into(),
            3600,
        ));
        mgr.save_to_file(&path).unwrap();

        let loaded = RoutineManager::load_from_file(&path).unwrap();
        assert_eq!(loaded.routines.len(), 2);
    }

    #[test]
    fn drain_due_workflow() {
        let mut mgr = RoutineManager::new();

        // Add routines with 0 interval (fire immediately)
        let mut r1 = ScheduledRoutine::new("instant-1".into(), "prompt-1".into(), 0);
        r1.max_fires = Some(1);
        let mut r2 = ScheduledRoutine::new("instant-2".into(), "prompt-2".into(), 0);
        r2.max_fires = Some(1);
        mgr.add(r1);
        mgr.add(r2);

        // First drain: both fire
        let due = mgr.drain_due();
        assert_eq!(due.len(), 2);

        // Second drain: neither fires (max_fires=1 reached)
        let due = mgr.drain_due();
        assert!(due.is_empty());
    }

    #[test]
    fn toggle_enables_disables() {
        let mut mgr = RoutineManager::new();
        let r = ScheduledRoutine::new("test".into(), "prompt".into(), 60);
        let id = r.id.clone();
        mgr.add(r);

        // Toggle off
        let state = mgr.toggle(&id);
        assert_eq!(state, Some(false));
        assert!(!mgr.get(&id).unwrap().enabled);

        // Toggle on
        let state = mgr.toggle(&id);
        assert_eq!(state, Some(true));
        assert!(mgr.get(&id).unwrap().enabled);
    }
}
