//! Tests for the hooks system.

use super::*;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

// ── HookEventType tests ──────────────────────────────────────────────

#[test]
fn test_hook_event_type_from_str() {
    assert_eq!(
        HookEventType::from_str_lossy("PreToolUse"),
        Some(HookEventType::PreToolUse)
    );
    assert_eq!(
        HookEventType::from_str_lossy("PostToolUse"),
        Some(HookEventType::PostToolUse)
    );
    assert_eq!(
        HookEventType::from_str_lossy("SessionStart"),
        Some(HookEventType::SessionStart)
    );
    assert_eq!(
        HookEventType::from_str_lossy("SessionEnd"),
        Some(HookEventType::SessionEnd)
    );
    assert_eq!(
        HookEventType::from_str_lossy("Notification"),
        Some(HookEventType::Notification)
    );
    assert_eq!(
        HookEventType::from_str_lossy("UserPromptSubmit"),
        Some(HookEventType::UserPromptSubmit)
    );
    assert_eq!(HookEventType::from_str_lossy("Unknown"), None);
}

#[test]
fn test_hook_event_type_display() {
    assert_eq!(HookEventType::PreToolUse.to_string(), "PreToolUse");
    assert_eq!(HookEventType::PostToolUse.to_string(), "PostToolUse");
    assert_eq!(HookEventType::SessionStart.to_string(), "SessionStart");
    assert_eq!(HookEventType::SessionEnd.to_string(), "SessionEnd");
    assert_eq!(HookEventType::Notification.to_string(), "Notification");
    assert_eq!(
        HookEventType::UserPromptSubmit.to_string(),
        "UserPromptSubmit"
    );
}

#[test]
fn test_hook_event_type_serialization() {
    let event_type = HookEventType::PreToolUse;
    let json = serde_json::to_string(&event_type).unwrap();
    assert_eq!(json, "\"PreToolUse\"");

    let parsed: HookEventType = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, HookEventType::PreToolUse);
}

// ── HookEvent tests ──────────────────────────────────────────────────

#[test]
fn test_hook_event_event_type() {
    let event = HookEvent::PreToolUse {
        tool_name: "Bash".to_string(),
        input: serde_json::json!({"command": "ls"}),
    };
    assert_eq!(event.event_type(), HookEventType::PreToolUse);

    let event = HookEvent::SessionStart {
        session_id: "abc123".to_string(),
    };
    assert_eq!(event.event_type(), HookEventType::SessionStart);
}

#[test]
fn test_hook_event_match_subject() {
    let event = HookEvent::PreToolUse {
        tool_name: "Bash".to_string(),
        input: serde_json::json!({}),
    };
    assert_eq!(event.match_subject(), "Bash");

    let event = HookEvent::SessionEnd {
        session_id: "sess-42".to_string(),
    };
    assert_eq!(event.match_subject(), "sess-42");

    let event = HookEvent::UserPromptSubmit {
        prompt: "Hello world".to_string(),
    };
    assert_eq!(event.match_subject(), "Hello world");
}

#[test]
fn test_hook_event_serialization() {
    let event = HookEvent::PreToolUse {
        tool_name: "Bash".to_string(),
        input: serde_json::json!({"command": "ls"}),
    };

    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("PreToolUse"));
    assert!(json.contains("Bash"));
    assert!(json.contains("command"));

    let parsed: HookEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.event_type(), HookEventType::PreToolUse);
}

// ── HookDecision tests ───────────────────────────────────────────────

#[test]
fn test_hook_decision_default() {
    assert_eq!(HookDecision::default(), HookDecision::Allow);
}

#[test]
fn test_hook_decision_serialization() {
    let decision = HookDecision::Deny {
        reason: "not allowed".to_string(),
    };
    let json = serde_json::to_string(&decision).unwrap();
    assert!(json.contains("Deny"));
    assert!(json.contains("not allowed"));

    let parsed: HookDecision = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, decision);
}

#[test]
fn test_hook_decision_modify_serialization() {
    let decision = HookDecision::Modify {
        modified_input: Some(serde_json::json!({"command": "echo hello"})),
        modified_output: None,
    };
    let json = serde_json::to_string(&decision).unwrap();
    assert!(json.contains("modified_input"));
    assert!(!json.contains("modified_output"));

    let parsed: HookDecision = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, decision);
}

// ── HookResult tests ─────────────────────────────────────────────────

#[test]
fn test_hook_result_parse_decision_empty() {
    let decision = HookResult::parse_decision("");
    assert_eq!(decision, HookDecision::Allow);
}

#[test]
fn test_hook_result_parse_decision_plain_text() {
    let decision = HookResult::parse_decision("Just some output\nMore output");
    assert_eq!(decision, HookDecision::Allow);
}

#[test]
fn test_hook_result_parse_decision_deny() {
    let stdout = r#"{"decision": "deny", "reason": "not allowed"}"#;
    let decision = HookResult::parse_decision(stdout);
    assert_eq!(
        decision,
        HookDecision::Deny {
            reason: "not allowed".to_string()
        }
    );
}

#[test]
fn test_hook_result_parse_decision_modify() {
    let stdout = r#"{"decision": "modify", "modified_input": {"command": "echo hello"}}"#;
    let decision = HookResult::parse_decision(stdout);
    assert_eq!(
        decision,
        HookDecision::Modify {
            modified_input: Some(serde_json::json!({"command": "echo hello"})),
            modified_output: None,
        }
    );
}

#[test]
fn test_hook_result_parse_decision_with_extra_lines() {
    let stdout = r#"{"decision": "deny", "reason": "blocked"}
Some debug output
More lines"#;
    let decision = HookResult::parse_decision(stdout);
    assert_eq!(
        decision,
        HookDecision::Deny {
            reason: "blocked".to_string()
        }
    );
}

#[test]
fn test_hook_result_parse_decision_invalid_json() {
    let stdout = "{invalid json";
    let decision = HookResult::parse_decision(stdout);
    assert_eq!(decision, HookDecision::Allow);
}

#[test]
fn test_hook_result_parse_decision_unknown_decision() {
    let stdout = r#"{"decision": "unknown"}"#;
    let decision = HookResult::parse_decision(stdout);
    assert_eq!(decision, HookDecision::Allow);
}

#[test]
fn test_hook_result_is_denied() {
    let result = HookResult {
        exit_code: 0,
        stdout: String::new(),
        stderr: String::new(),
        decision: HookDecision::Deny {
            reason: "test".to_string(),
        },
        command: "test".to_string(),
    };
    assert!(result.is_denied());
    assert!(!result.has_modifications());
}

#[test]
fn test_hook_result_has_modifications() {
    let result = HookResult {
        exit_code: 0,
        stdout: String::new(),
        stderr: String::new(),
        decision: HookDecision::Modify {
            modified_input: None,
            modified_output: None,
        },
        command: "test".to_string(),
    };
    assert!(!result.is_denied());
    assert!(result.has_modifications());
}

// ── HookDef tests ────────────────────────────────────────────────────

#[test]
fn test_hook_def_new() {
    let hook = HookDef::new("echo hello");
    assert_eq!(hook.command, "echo hello");
    assert_eq!(hook.timeout, 30);
    assert!(hook.blocking);
}

#[test]
fn test_hook_def_builder() {
    let hook = HookDef::new("echo hello")
        .with_timeout(10)
        .with_blocking(false);
    assert_eq!(hook.timeout, 10);
    assert!(!hook.blocking);
}

// ── Pipe-separated matcher tests ────────────────────────────────────

#[test]
fn test_matcher_pipe_separated_match() {
    let config = HookConfig::new("Edit|Write");
    assert!(config.matches("Edit"));
    assert!(config.matches("Write"));
    assert!(!config.matches("Bash"));
}

#[test]
fn test_matcher_pipe_separated_with_wildcard() {
    let config = HookConfig::new("Bash|*");
    assert!(config.matches("Bash"));
    assert!(config.matches("Edit"));
    assert!(config.matches("Anything"));
}

#[test]
fn test_matcher_single_no_pipe() {
    let config = HookConfig::new("Bash");
    assert!(config.matches("Bash"));
    assert!(!config.matches("Edit"));
}

#[test]
fn test_matcher_wildcard() {
    let config = HookConfig::new("*");
    assert!(config.matches("Bash"));
    assert!(config.matches("Edit"));
    assert!(config.matches("Anything"));
}

#[test]
fn test_hook_config_with_condition() {
    let config = HookConfig::new("Bash").with_condition("Bash(rm *)");
    assert_eq!(config.if_condition.as_deref(), Some("Bash(rm *)"));
}

#[test]
fn test_hook_def_timeout_duration() {
    let hook = HookDef::new("echo hello").with_timeout(5);
    assert_eq!(hook.timeout_duration(), Duration::from_secs(5));
}

#[test]
fn test_hook_def_deserialization_defaults() {
    let json = r#"{"command": "echo hello"}"#;
    let hook: HookDef = serde_json::from_str(json).unwrap();
    assert_eq!(hook.command, "echo hello");
    assert_eq!(hook.timeout, 30);
    assert!(hook.blocking);
}

// ── HookConfig tests ─────────────────────────────────────────────────

#[test]
fn test_hook_config_wildcard_match() {
    let config = HookConfig::new("*");
    assert!(config.matches("Bash"));
    assert!(config.matches("Read"));
    assert!(config.matches(""));
}

#[test]
fn test_hook_config_exact_match() {
    let config = HookConfig::new("Bash");
    assert!(config.matches("Bash"));
    assert!(!config.matches("Read"));
    assert!(!config.matches("BashTool"));
}

#[test]
fn test_hook_config_regex_match() {
    let config = HookConfig::new("Bash.*");
    assert!(config.matches("Bash"));
    assert!(config.matches("BashTool"));
    assert!(!config.matches("Read"));
}

#[test]
fn test_hook_config_fallback_substring_match() {
    // An invalid regex falls back to substring matching
    let config = HookConfig::new("[invalid");
    assert!(config.matches("[invalid"));
    assert!(config.matches("some[invalid]thing"));
    assert!(!config.matches("something-else"));
}

#[test]
fn test_hook_config_builder() {
    let config = HookConfig::new("Bash")
        .with_hook(HookDef::new("echo pre"))
        .with_hook(HookDef::new("echo post").with_blocking(false));

    assert_eq!(config.matcher, "Bash");
    assert_eq!(config.hooks.len(), 2);
    assert!(config.hooks[0].blocking);
    assert!(!config.hooks[1].blocking);
}

// ── HooksFile tests ──────────────────────────────────────────────────

#[test]
fn test_hooks_file_new() {
    let file = HooksFile::new();
    assert!(file.hooks.is_empty());
}

#[test]
fn test_hooks_file_from_json() {
    let json = r#"{
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash",
                        "hooks": [
                            {"command": "echo pre", "timeout": 5, "blocking": false}
                        ]
                    }
                ],
                "SessionStart": [
                    {
                        "matcher": "*",
                        "hooks": [
                            {"command": "echo started"}
                        ]
                    }
                ]
            }
        }"#;

    let file = HooksFile::from_json(json).unwrap();
    assert_eq!(file.hooks.len(), 2);

    let pre_tool = file.hooks.get("PreToolUse").unwrap();
    assert_eq!(pre_tool.len(), 1);
    assert_eq!(pre_tool[0].matcher, "Bash");
    assert_eq!(pre_tool[0].hooks.len(), 1);
    assert_eq!(pre_tool[0].hooks[0].command, "echo pre");
    assert_eq!(pre_tool[0].hooks[0].timeout, 5);
    assert!(!pre_tool[0].hooks[0].blocking);

    let session = file.hooks.get("SessionStart").unwrap();
    assert_eq!(session.len(), 1);
    assert_eq!(session[0].matcher, "*");
    assert_eq!(session[0].hooks[0].timeout, 30);
    assert!(session[0].hooks[0].blocking);
}

#[test]
fn test_hooks_file_get_for_event() {
    let mut file = HooksFile::new();
    file.hooks
        .insert("PreToolUse".to_string(), vec![HookConfig::new("Bash")]);

    let configs = file.get_for_event(&HookEventType::PreToolUse);
    assert_eq!(configs.len(), 1);

    let configs = file.get_for_event(&HookEventType::SessionStart);
    assert!(configs.is_empty());
}

#[test]
fn test_hooks_file_merge() {
    let mut file1 = HooksFile::new();
    file1
        .hooks
        .insert("PreToolUse".to_string(), vec![HookConfig::new("Bash")]);

    let mut file2 = HooksFile::new();
    file2
        .hooks
        .insert("PreToolUse".to_string(), vec![HookConfig::new("Read")]);
    file2
        .hooks
        .insert("SessionStart".to_string(), vec![HookConfig::new("*")]);

    file1.merge(file2);

    let pre = file1.hooks.get("PreToolUse").unwrap();
    assert_eq!(pre.len(), 2);
    assert_eq!(pre[0].matcher, "Bash");
    assert_eq!(pre[1].matcher, "Read");

    let session = file1.hooks.get("SessionStart").unwrap();
    assert_eq!(session.len(), 1);
}

#[test]
fn test_hooks_file_to_json() {
    let file = HooksFile::new();
    let json = file.to_json().unwrap();
    let parsed: Value = serde_json::from_str(&json).unwrap();
    assert!(parsed["hooks"].is_object());
}

#[test]
fn test_hooks_file_load_nonexistent() {
    let file = HooksFile::load_from_file(Path::new("/nonexistent/path/hooks.json"));
    assert!(file.is_ok());
    assert!(file.unwrap().hooks.is_empty());
}

#[test]
fn test_hooks_file_invalid_json() {
    let result = HooksFile::from_json("not json");
    assert!(result.is_err());
}

// ── HookManager tests ────────────────────────────────────────────────

#[tokio::test]
async fn test_hook_manager_new() {
    let manager = HookManager::new();
    assert_eq!(
        manager.user_config_path().file_name().unwrap(),
        "hooks.json"
    );
    assert!(
        manager
            .project_config_path()
            .ends_with(".shannon/hooks.json")
    );
}

#[tokio::test]
async fn test_hook_manager_load_nonexistent() {
    let mut manager = HookManager::with_paths(
        PathBuf::from("/nonexistent/user_hooks.json"),
        PathBuf::from("/nonexistent/project_hooks.json"),
    );

    assert!(manager.load().is_ok());
    assert!(manager.configured_event_types().is_empty());
}

#[tokio::test]
async fn test_hook_manager_load_from_file() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let hooks_path = temp_dir.path().join("hooks.json");

    let json = r#"{
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash",
                        "hooks": [
                            {"command": "echo pre-bash", "timeout": 5, "blocking": true}
                        ]
                    }
                ]
            }
        }"#;

    std::fs::write(&hooks_path, json).unwrap();

    let mut manager =
        HookManager::with_paths(PathBuf::from("/nonexistent"), PathBuf::from("/nonexistent"));
    manager.load_from_path(&hooks_path).unwrap();

    let event_types = manager.configured_event_types();
    assert_eq!(event_types.len(), 1);
    assert_eq!(event_types[0], HookEventType::PreToolUse);
}

#[tokio::test]
async fn test_hook_manager_run_hooks_blocking() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let hooks_path = temp_dir.path().join("hooks.json");

    let json = r#"{
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash",
                        "hooks": [
                            {"command": "echo pre-tool", "timeout": 5, "blocking": true}
                        ]
                    }
                ]
            }
        }"#;

    std::fs::write(&hooks_path, json).unwrap();

    let mut manager =
        HookManager::with_paths(PathBuf::from("/nonexistent"), PathBuf::from("/nonexistent"));
    manager.load_from_path(&hooks_path).unwrap();

    let event = HookEvent::PreToolUse {
        tool_name: "Bash".to_string(),
        input: serde_json::json!({"command": "ls"}),
    };

    let results = manager.run_hooks(&event).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].exit_code, 0);
    assert!(results[0].stdout.contains("pre-tool"));
    assert_eq!(results[0].decision, HookDecision::Allow);
}

#[tokio::test]
async fn test_hook_manager_run_hooks_no_match() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let hooks_path = temp_dir.path().join("hooks.json");

    let json = r#"{
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Read",
                        "hooks": [
                            {"command": "echo read-hook", "timeout": 5, "blocking": true}
                        ]
                    }
                ]
            }
        }"#;

    std::fs::write(&hooks_path, json).unwrap();

    let mut manager =
        HookManager::with_paths(PathBuf::from("/nonexistent"), PathBuf::from("/nonexistent"));
    manager.load_from_path(&hooks_path).unwrap();

    let event = HookEvent::PreToolUse {
        tool_name: "Bash".to_string(),
        input: serde_json::json!({"command": "ls"}),
    };

    let results = manager.run_hooks(&event).await.unwrap();
    assert!(results.is_empty());
}

#[tokio::test]
async fn test_hook_manager_run_hooks_deny_decision() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let hooks_path = temp_dir.path().join("hooks.json");

    let json = r#"{
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "*",
                        "hooks": [
                            {"command": "echo '{\"decision\": \"deny\", \"reason\": \"blocked by policy\"}'", "timeout": 5, "blocking": true}
                        ]
                    }
                ]
            }
        }"#;

    std::fs::write(&hooks_path, json).unwrap();

    let mut manager =
        HookManager::with_paths(PathBuf::from("/nonexistent"), PathBuf::from("/nonexistent"));
    manager.load_from_path(&hooks_path).unwrap();

    let event = HookEvent::PreToolUse {
        tool_name: "Bash".to_string(),
        input: serde_json::json!({"command": "rm -rf /"}),
    };

    let results = manager.run_hooks(&event).await.unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].is_denied());
}

#[tokio::test]
async fn test_hook_manager_run_hooks_timeout() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let hooks_path = temp_dir.path().join("hooks.json");

    // Register hook under SessionStart (matching the event we'll fire)
    let json = r#"{
            "hooks": {
                "SessionStart": [
                    {
                        "matcher": "*",
                        "hooks": [
                            {"command": "sleep 60", "timeout": 1, "blocking": true}
                        ]
                    }
                ]
            }
        }"#;

    std::fs::write(&hooks_path, json).unwrap();

    let mut manager =
        HookManager::with_paths(PathBuf::from("/nonexistent"), PathBuf::from("/nonexistent"));
    manager.load_from_path(&hooks_path).unwrap();

    let event = HookEvent::SessionStart {
        session_id: "test".to_string(),
    };

    let result = manager.run_hooks(&event).await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("timed out"),
        "Error should mention timeout: {err}"
    );
}

#[tokio::test]
async fn test_hook_manager_run_hooks_non_blocking() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let hooks_path = temp_dir.path().join("hooks.json");

    // Non-blocking hooks should not appear in results
    let json = r#"{
            "hooks": {
                "SessionStart": [
                    {
                        "matcher": "*",
                        "hooks": [
                            {"command": "echo 'non-blocking'", "timeout": 5, "blocking": false}
                        ]
                    }
                ]
            }
        }"#;

    std::fs::write(&hooks_path, json).unwrap();

    let mut manager =
        HookManager::with_paths(PathBuf::from("/nonexistent"), PathBuf::from("/nonexistent"));
    manager.load_from_path(&hooks_path).unwrap();

    let event = HookEvent::SessionStart {
        session_id: "test".to_string(),
    };

    let results = manager.run_hooks(&event).await.unwrap();
    assert!(results.is_empty());
}

#[tokio::test]
async fn test_hook_manager_run_hooks_wildcard_match() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let hooks_path = temp_dir.path().join("hooks.json");

    let json = r#"{
            "hooks": {
                "SessionStart": [
                    {
                        "matcher": "*",
                        "hooks": [
                            {"command": "echo 'session started'", "timeout": 5, "blocking": true}
                        ]
                    }
                ]
            }
        }"#;

    std::fs::write(&hooks_path, json).unwrap();

    let mut manager =
        HookManager::with_paths(PathBuf::from("/nonexistent"), PathBuf::from("/nonexistent"));
    manager.load_from_path(&hooks_path).unwrap();

    let event = HookEvent::SessionStart {
        session_id: "any-session-id".to_string(),
    };

    let results = manager.run_hooks(&event).await.unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].stdout.contains("session started"));
}

// ── resolve_results tests ────────────────────────────────────────────

#[test]
fn test_resolve_results_all_allow() {
    let results = vec![
        HookResult {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
            decision: HookDecision::Allow,
            command: "cmd1".to_string(),
        },
        HookResult {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
            decision: HookDecision::Allow,
            command: "cmd2".to_string(),
        },
    ];
    assert_eq!(HookManager::resolve_results(&results), HookDecision::Allow);
}

#[test]
fn test_resolve_results_first_deny_wins() {
    let results = vec![
        HookResult {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
            decision: HookDecision::Allow,
            command: "cmd1".to_string(),
        },
        HookResult {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
            decision: HookDecision::Deny {
                reason: "first deny".to_string(),
            },
            command: "cmd2".to_string(),
        },
        HookResult {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
            decision: HookDecision::Deny {
                reason: "second deny".to_string(),
            },
            command: "cmd3".to_string(),
        },
    ];
    let resolved = HookManager::resolve_results(&results);
    assert_eq!(
        resolved,
        HookDecision::Deny {
            reason: "first deny".to_string()
        }
    );
}

#[test]
fn test_resolve_results_last_modify_wins() {
    let results = vec![
        HookResult {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
            decision: HookDecision::Modify {
                modified_input: Some(serde_json::json!({"v": 1})),
                modified_output: None,
            },
            command: "cmd1".to_string(),
        },
        HookResult {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
            decision: HookDecision::Modify {
                modified_input: Some(serde_json::json!({"v": 2})),
                modified_output: None,
            },
            command: "cmd2".to_string(),
        },
    ];
    let resolved = HookManager::resolve_results(&results);
    assert_eq!(
        resolved,
        HookDecision::Modify {
            modified_input: Some(serde_json::json!({"v": 2})),
            modified_output: None,
        }
    );
}

#[test]
fn test_resolve_results_deny_overrides_modify() {
    let results = vec![
        HookResult {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
            decision: HookDecision::Modify {
                modified_input: Some(serde_json::json!({"v": 1})),
                modified_output: None,
            },
            command: "cmd1".to_string(),
        },
        HookResult {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
            decision: HookDecision::Deny {
                reason: "blocked".to_string(),
            },
            command: "cmd2".to_string(),
        },
    ];
    let resolved = HookManager::resolve_results(&results);
    assert_eq!(
        resolved,
        HookDecision::Deny {
            reason: "blocked".to_string()
        }
    );
}

#[test]
fn test_resolve_results_empty() {
    let results: Vec<HookResult> = vec![];
    assert_eq!(HookManager::resolve_results(&results), HookDecision::Allow);
}

// ── HookManager merge/load tests ─────────────────────────────────────

#[tokio::test]
async fn test_hook_manager_merge_user_and_project() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let user_path = temp_dir.path().join("user_hooks.json");
    let project_path = temp_dir.path().join("project_hooks.json");

    let user_json = r#"{
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash",
                        "hooks": [
                            {"command": "echo 'user hook'", "timeout": 5, "blocking": true}
                        ]
                    }
                ]
            }
        }"#;

    let project_json = r#"{
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Read",
                        "hooks": [
                            {"command": "echo 'project hook'", "timeout": 5, "blocking": true}
                        ]
                    }
                ]
            }
        }"#;

    std::fs::write(&user_path, user_json).unwrap();
    std::fs::write(&project_path, project_json).unwrap();

    let mut manager = HookManager::with_paths(user_path, project_path);
    manager.load().unwrap();

    // Both user and project hooks should be present
    let configs = manager
        .hooks_file()
        .get_for_event(&HookEventType::PreToolUse);
    assert_eq!(configs.len(), 2);
}

// ── Integration: full flow test ──────────────────────────────────────

#[tokio::test]
async fn test_full_flow_with_modify_decision() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let hooks_path = temp_dir.path().join("hooks.json");

    // Hook that modifies the tool input
    let json = r#"{
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash",
                        "hooks": [
                            {"command": "echo '{\"decision\": \"modify\", \"modified_input\": {\"command\": \"echo safe\"}}'", "timeout": 5, "blocking": true}
                        ]
                    }
                ]
            }
        }"#;

    std::fs::write(&hooks_path, json).unwrap();

    let mut manager =
        HookManager::with_paths(PathBuf::from("/nonexistent"), PathBuf::from("/nonexistent"));
    manager.load_from_path(&hooks_path).unwrap();

    let event = HookEvent::PreToolUse {
        tool_name: "Bash".to_string(),
        input: serde_json::json!({"command": "rm -rf /"}),
    };

    let results = manager.run_hooks(&event).await.unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].has_modifications());

    let resolved = HookManager::resolve_results(&results);
    if let HookDecision::Modify { modified_input, .. } = resolved {
        assert_eq!(
            modified_input,
            Some(serde_json::json!({"command": "echo safe"}))
        );
    } else {
        panic!("Expected Modify decision, got {resolved:?}");
    }
}

// ── Edge case tests ──────────────────────────────────────────────────

#[test]
fn test_hook_event_to_json_bytes() {
    let event = HookEvent::Notification {
        message: "test message".to_string(),
    };
    let bytes = event.to_json_bytes();
    let parsed: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(parsed["Notification"]["message"], "test message");
}

#[test]
fn test_hook_config_serialization_camel_case() {
    let config = HookConfig {
        matcher: "Bash".to_string(),
        if_condition: None,
        hooks: vec![HookDef {
            command: "echo test".to_string(),
            r#type: HookType::Command,
            url: None,
            headers: HashMap::new(),
            timeout: 10,
            blocking: false,
            allowed_env_vars: Vec::new(),
            shell: None,
            prompt_template: None,
            model: None,
        }],
    };

    let json = serde_json::to_string(&config).unwrap();
    assert!(json.contains("matcher"));
    assert!(json.contains("hooks"));
    assert!(json.contains("command"));
    assert!(json.contains("timeout"));
    assert!(json.contains("blocking"));

    // camelCase is handled by serde rename_all
    let parsed: HookConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.matcher, "Bash");
    assert_eq!(parsed.hooks.len(), 1);
}

#[test]
fn test_hook_error_display() {
    let err = HookError::Timeout {
        command: "sleep 100".to_string(),
        timeout_secs: 5,
    };
    assert!(err.to_string().contains("timed out"));
    assert!(err.to_string().contains("5"));

    let err = HookError::Denied {
        reason: "policy".to_string(),
    };
    assert!(err.to_string().contains("policy"));
}

// ── Claude Code compatible path loading ─────────────────────────────

#[test]
fn test_hooks_file_ignores_non_hook_fields() {
    // Claude Code settings.json has extra fields like mcpServers, permissions
    let json = r#"{
            "mcpServers": {
                "fetch": {"command": "uvx", "args": ["mcp-server-fetch"]}
            },
            "permissions": {"allow": ["Bash"]},
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash",
                        "hooks": [
                            {"command": "echo 'hook ran'", "timeout": 5, "blocking": true}
                        ]
                    }
                ]
            }
        }"#;
    let file = HooksFile::from_json(json).unwrap();
    let configs = file.get_for_event(&HookEventType::PreToolUse);
    assert_eq!(configs.len(), 1);
    assert_eq!(configs[0].matcher, "Bash");
}

#[tokio::test]
async fn test_load_from_claude_code_settings_paths() {
    let temp_dir = tempfile::TempDir::new().unwrap();

    // Create .claude/settings.json in the temp dir (project-level)
    let claude_dir = temp_dir.path().join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    let claude_settings = claude_dir.join("settings.json");
    std::fs::write(&claude_settings, r#"{
            "hooks": {
                "SessionStart": [
                    {"matcher": "*", "hooks": [{"command": "echo 'claude session start'", "timeout": 5}]}
                ]
            },
            "mcpServers": {}
        }"#).unwrap();

    // Also create .shannon/hooks.json (project-level via base_dir)
    let shannon_dir = temp_dir.path().join(".shannon");
    std::fs::create_dir_all(&shannon_dir).unwrap();
    let shannon_hooks = shannon_dir.join("hooks.json");
    std::fs::write(
        &shannon_hooks,
        r#"{
            "hooks": {
                "PreToolUse": [
                    {"matcher": "Bash", "hooks": [{"command": "echo 'shannon hook'", "timeout": 5}]}
                ]
            }
        }"#,
    )
    .unwrap();

    let mut manager = HookManager::with_base_dir(temp_dir.path().to_path_buf());
    manager.load().unwrap();

    // Both .claude/settings.json and .shannon/hooks.json should be loaded
    let start_configs = manager
        .hooks_file()
        .get_for_event(&HookEventType::SessionStart);
    assert_eq!(start_configs.len(), 1);

    let pre_configs = manager
        .hooks_file()
        .get_for_event(&HookEventType::PreToolUse);
    assert_eq!(pre_configs.len(), 1);
}

#[tokio::test]
async fn test_load_priority_later_overrides() {
    let temp_dir = tempfile::TempDir::new().unwrap();

    // Create .claude/settings.local.json (highest project priority)
    let claude_dir = temp_dir.path().join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    let local_settings = claude_dir.join("settings.local.json");
    std::fs::write(&local_settings, r#"{
            "hooks": {
                "PreToolUse": [
                    {"matcher": "Bash", "hooks": [{"command": "echo 'local override'", "timeout": 5}]}
                ]
            }
        }"#).unwrap();

    let mut manager = HookManager::with_base_dir(temp_dir.path().to_path_buf());
    manager.load().unwrap();

    let configs = manager
        .hooks_file()
        .get_for_event(&HookEventType::PreToolUse);
    assert_eq!(configs.len(), 1);
    assert_eq!(configs[0].hooks[0].command, "echo 'local override'");
}
