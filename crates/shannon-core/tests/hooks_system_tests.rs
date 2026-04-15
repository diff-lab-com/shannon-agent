//! Comprehensive unit tests for the hooks system module.
//!
//! Covers all public types and their behavior:
//! - HookEventType: variants, from_str_lossy, Display, serde round-trip
//! - HookEvent: all variants, event_type(), match_subject(), to_json_bytes()
//! - HookDecision: all variants, Default
//! - HookResult: parse_decision(), is_denied(), has_modifications()
//! - HookDef: new(), with_timeout(), with_blocking(), timeout_duration()
//! - HookConfig: new(), with_hook(), matches() with exact/glob/regex/substring
//! - HooksFile: new(), from_json(), to_json(), get_for_event(), merge(), load_from_file()
//! - HookManager: new(), with_paths(), load(), load_from_path(), hooks_file(),
//!   configured_event_types(), resolve_results()

use serde_json::json;
use shannon_core::hooks::*;
use std::path::PathBuf;

// ============================================================================
// HookEventType Tests
// ============================================================================

mod hook_event_type_tests {
    use super::*;

    // -- from_str_lossy: valid strings ----------------------------------------

    #[test]
    fn from_str_lossy_pre_tool_use() {
        assert_eq!(
            HookEventType::from_str_lossy("PreToolUse"),
            Some(HookEventType::PreToolUse)
        );
    }

    #[test]
    fn from_str_lossy_post_tool_use() {
        assert_eq!(
            HookEventType::from_str_lossy("PostToolUse"),
            Some(HookEventType::PostToolUse)
        );
    }

    #[test]
    fn from_str_lossy_session_start() {
        assert_eq!(
            HookEventType::from_str_lossy("SessionStart"),
            Some(HookEventType::SessionStart)
        );
    }

    #[test]
    fn from_str_lossy_session_end() {
        assert_eq!(
            HookEventType::from_str_lossy("SessionEnd"),
            Some(HookEventType::SessionEnd)
        );
    }

    #[test]
    fn from_str_lossy_notification() {
        assert_eq!(
            HookEventType::from_str_lossy("Notification"),
            Some(HookEventType::Notification)
        );
    }

    #[test]
    fn from_str_lossy_user_prompt_submit() {
        assert_eq!(
            HookEventType::from_str_lossy("UserPromptSubmit"),
            Some(HookEventType::UserPromptSubmit)
        );
    }

    // -- from_str_lossy: invalid strings return None --------------------------

    #[test]
    fn from_str_lossy_unknown_returns_none() {
        assert_eq!(HookEventType::from_str_lossy("Unknown"), None);
    }

    #[test]
    fn from_str_lossy_empty_returns_none() {
        assert_eq!(HookEventType::from_str_lossy(""), None);
    }

    #[test]
    fn from_str_lossy_lowercase_returns_none() {
        assert_eq!(HookEventType::from_str_lossy("pretooluse"), None);
    }

    #[test]
    fn from_str_lossy_snake_case_returns_none() {
        assert_eq!(HookEventType::from_str_lossy("pre_tool_use"), None);
    }

    #[test]
    fn from_str_lossy_partial_name_returns_none() {
        assert_eq!(HookEventType::from_str_lossy("PreTool"), None);
    }

    // -- Display --------------------------------------------------------------

    #[test]
    fn display_pre_tool_use() {
        assert_eq!(HookEventType::PreToolUse.to_string(), "PreToolUse");
    }

    #[test]
    fn display_post_tool_use() {
        assert_eq!(HookEventType::PostToolUse.to_string(), "PostToolUse");
    }

    #[test]
    fn display_session_start() {
        assert_eq!(HookEventType::SessionStart.to_string(), "SessionStart");
    }

    #[test]
    fn display_session_end() {
        assert_eq!(HookEventType::SessionEnd.to_string(), "SessionEnd");
    }

    #[test]
    fn display_notification() {
        assert_eq!(HookEventType::Notification.to_string(), "Notification");
    }

    #[test]
    fn display_user_prompt_submit() {
        assert_eq!(
            HookEventType::UserPromptSubmit.to_string(),
            "UserPromptSubmit"
        );
    }

    // -- Serde round-trip (Serialize + Deserialize) ---------------------------

    #[test]
    fn serde_round_trip_all_variants() {
        let variants = [
            HookEventType::PreToolUse,
            HookEventType::PostToolUse,
            HookEventType::SessionStart,
            HookEventType::SessionEnd,
            HookEventType::Notification,
            HookEventType::UserPromptSubmit,
        ];

        for variant in &variants {
            let json = serde_json::to_string(variant).unwrap();
            let parsed: HookEventType = serde_json::from_str(&json).unwrap();
            assert_eq!(*variant, parsed, "Round-trip failed for {:?}", variant);
        }
    }

    #[test]
    fn serde_renames_to_pascal_case() {
        let json = serde_json::to_string(&HookEventType::PreToolUse).unwrap();
        assert_eq!(json, "\"PreToolUse\"");

        let json = serde_json::to_string(&HookEventType::UserPromptSubmit).unwrap();
        assert_eq!(json, "\"UserPromptSubmit\"");
    }

    #[test]
    fn serde_rejects_invalid_variant() {
        let result = serde_json::from_str::<HookEventType>("\"InvalidType\"");
        assert!(result.is_err());
    }

    // -- Equality and hashing --------------------------------------------------

    #[test]
    fn equality_same_variant() {
        assert_eq!(HookEventType::PreToolUse, HookEventType::PreToolUse);
    }

    #[test]
    fn inequality_different_variants() {
        assert_ne!(HookEventType::PreToolUse, HookEventType::PostToolUse);
    }

    #[test]
    fn can_be_used_as_hashmap_key() {
        let mut map = std::collections::HashMap::new();
        map.insert(HookEventType::PreToolUse, "before");
        map.insert(HookEventType::PostToolUse, "after");

        assert_eq!(map.get(&HookEventType::PreToolUse), Some(&"before"));
        assert_eq!(map.get(&HookEventType::PostToolUse), Some(&"after"));
        assert_eq!(map.get(&HookEventType::SessionStart), None);
    }
}

// ============================================================================
// HookEvent Tests
// ============================================================================

mod hook_event_tests {
    use super::*;

    // -- All variants can be constructed --------------------------------------

    #[test]
    fn pre_tool_use_construction() {
        let event = HookEvent::PreToolUse {
            tool_name: "Bash".to_string(),
            input: json!({"command": "ls -la"}),
        };
        assert_eq!(event.event_type(), HookEventType::PreToolUse);
    }

    #[test]
    fn post_tool_use_construction() {
        let event = HookEvent::PostToolUse {
            tool_name: "Read".to_string(),
            input: json!({"path": "/tmp/file.txt"}),
            output: json!({"content": "hello"}),
        };
        assert_eq!(event.event_type(), HookEventType::PostToolUse);
    }

    #[test]
    fn session_start_construction() {
        let event = HookEvent::SessionStart {
            session_id: "sess-001".to_string(),
        };
        assert_eq!(event.event_type(), HookEventType::SessionStart);
    }

    #[test]
    fn session_end_construction() {
        let event = HookEvent::SessionEnd {
            session_id: "sess-001".to_string(),
        };
        assert_eq!(event.event_type(), HookEventType::SessionEnd);
    }

    #[test]
    fn notification_construction() {
        let event = HookEvent::Notification {
            message: "Build complete".to_string(),
        };
        assert_eq!(event.event_type(), HookEventType::Notification);
    }

    #[test]
    fn user_prompt_submit_construction() {
        let event = HookEvent::UserPromptSubmit {
            prompt: "Fix the bug".to_string(),
        };
        assert_eq!(event.event_type(), HookEventType::UserPromptSubmit);
    }

    // -- event_type() returns correct HookEventType for each variant ----------

    #[test]
    fn event_type_pre_tool_use() {
        let event = HookEvent::PreToolUse {
            tool_name: "Bash".into(),
            input: json!(null),
        };
        assert_eq!(event.event_type(), HookEventType::PreToolUse);
    }

    #[test]
    fn event_type_post_tool_use() {
        let event = HookEvent::PostToolUse {
            tool_name: "Write".into(),
            input: json!(null),
            output: json!(null),
        };
        assert_eq!(event.event_type(), HookEventType::PostToolUse);
    }

    #[test]
    fn event_type_session_start() {
        let event = HookEvent::SessionStart {
            session_id: "abc".into(),
        };
        assert_eq!(event.event_type(), HookEventType::SessionStart);
    }

    #[test]
    fn event_type_session_end() {
        let event = HookEvent::SessionEnd {
            session_id: "abc".into(),
        };
        assert_eq!(event.event_type(), HookEventType::SessionEnd);
    }

    #[test]
    fn event_type_notification() {
        let event = HookEvent::Notification {
            message: "msg".into(),
        };
        assert_eq!(event.event_type(), HookEventType::Notification);
    }

    #[test]
    fn event_type_user_prompt_submit() {
        let event = HookEvent::UserPromptSubmit {
            prompt: "hi".into(),
        };
        assert_eq!(event.event_type(), HookEventType::UserPromptSubmit);
    }

    // -- match_subject() returns correct strings ------------------------------

    #[test]
    fn match_subject_pre_tool_use_returns_tool_name() {
        let event = HookEvent::PreToolUse {
            tool_name: "Bash".to_string(),
            input: json!({}),
        };
        assert_eq!(event.match_subject(), "Bash");
    }

    #[test]
    fn match_subject_post_tool_use_returns_tool_name() {
        let event = HookEvent::PostToolUse {
            tool_name: "Read".to_string(),
            input: json!({}),
            output: json!({}),
        };
        assert_eq!(event.match_subject(), "Read");
    }

    #[test]
    fn match_subject_session_start_returns_session_id() {
        let event = HookEvent::SessionStart {
            session_id: "sess-42".to_string(),
        };
        assert_eq!(event.match_subject(), "sess-42");
    }

    #[test]
    fn match_subject_session_end_returns_session_id() {
        let event = HookEvent::SessionEnd {
            session_id: "sess-99".to_string(),
        };
        assert_eq!(event.match_subject(), "sess-99");
    }

    #[test]
    fn match_subject_notification_returns_message() {
        let event = HookEvent::Notification {
            message: "Deploy succeeded".to_string(),
        };
        assert_eq!(event.match_subject(), "Deploy succeeded");
    }

    #[test]
    fn match_subject_user_prompt_submit_returns_prompt() {
        let event = HookEvent::UserPromptSubmit {
            prompt: "Write a test".to_string(),
        };
        assert_eq!(event.match_subject(), "Write a test");
    }

    // -- to_json_bytes() produces valid JSON ----------------------------------

    #[test]
    fn to_json_bytes_pre_tool_use() {
        let event = HookEvent::PreToolUse {
            tool_name: "Bash".to_string(),
            input: json!({"command": "ls"}),
        };
        let bytes = event.to_json_bytes();
        assert!(!bytes.is_empty());

        let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(parsed["PreToolUse"]["tool_name"].is_string());
        assert_eq!(parsed["PreToolUse"]["tool_name"], "Bash");
        assert!(parsed["PreToolUse"]["input"]["command"].is_string());
    }

    #[test]
    fn to_json_bytes_post_tool_use() {
        let event = HookEvent::PostToolUse {
            tool_name: "Read".to_string(),
            input: json!({"path": "/etc/hosts"}),
            output: json!({"lines": 10}),
        };
        let bytes = event.to_json_bytes();
        let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(parsed["PostToolUse"]["tool_name"].is_string());
        assert!(parsed["PostToolUse"]["input"].is_object());
        assert!(parsed["PostToolUse"]["output"].is_object());
    }

    #[test]
    fn to_json_bytes_session_start() {
        let event = HookEvent::SessionStart {
            session_id: "sess-123".to_string(),
        };
        let bytes = event.to_json_bytes();
        let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed["SessionStart"]["session_id"], "sess-123");
    }

    #[test]
    fn to_json_bytes_session_end() {
        let event = HookEvent::SessionEnd {
            session_id: "sess-456".to_string(),
        };
        let bytes = event.to_json_bytes();
        let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed["SessionEnd"]["session_id"], "sess-456");
    }

    #[test]
    fn to_json_bytes_notification() {
        let event = HookEvent::Notification {
            message: "hello".to_string(),
        };
        let bytes = event.to_json_bytes();
        let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed["Notification"]["message"], "hello");
    }

    #[test]
    fn to_json_bytes_user_prompt_submit() {
        let event = HookEvent::UserPromptSubmit {
            prompt: "explain this".to_string(),
        };
        let bytes = event.to_json_bytes();
        let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed["UserPromptSubmit"]["prompt"], "explain this");
    }

    // -- Serde round-trip for HookEvent ---------------------------------------

    #[test]
    fn serde_round_trip_pre_tool_use() {
        let event = HookEvent::PreToolUse {
            tool_name: "Bash".to_string(),
            input: json!({"cmd": "test"}),
        };
        let json_str = serde_json::to_string(&event).unwrap();
        let parsed: HookEvent = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed.event_type(), HookEventType::PreToolUse);
        assert_eq!(parsed.match_subject(), "Bash");
    }

    #[test]
    fn serde_round_trip_all_variants() {
        let events = vec![
            HookEvent::PreToolUse {
                tool_name: "T".into(),
                input: json!(1),
            },
            HookEvent::PostToolUse {
                tool_name: "T".into(),
                input: json!(1),
                output: json!(2),
            },
            HookEvent::SessionStart {
                session_id: "s".into(),
            },
            HookEvent::SessionEnd {
                session_id: "s".into(),
            },
            HookEvent::Notification {
                message: "m".into(),
            },
            HookEvent::UserPromptSubmit {
                prompt: "p".into(),
            },
        ];

        for event in &events {
            let json_str = serde_json::to_string(event).unwrap();
            let bytes = event.to_json_bytes();

            // Both serialization methods should produce valid JSON
            let from_str: HookEvent = serde_json::from_str(&json_str).unwrap();
            let from_bytes: HookEvent = serde_json::from_slice(&bytes).unwrap();

            assert_eq!(
                from_str.event_type(),
                event.event_type(),
                "String serde round-trip failed for {:?}",
                event.event_type()
            );
            assert_eq!(
                from_bytes.event_type(),
                event.event_type(),
                "Bytes serde round-trip failed for {:?}",
                event.event_type()
            );
        }
    }
}

// ============================================================================
// HookDecision Tests
// ============================================================================

mod hook_decision_tests {
    use super::*;

    #[test]
    fn default_is_allow() {
        assert_eq!(HookDecision::default(), HookDecision::Allow);
    }

    #[test]
    fn deny_variant_constructs_with_reason() {
        let decision = HookDecision::Deny {
            reason: "forbidden".to_string(),
        };
        if let HookDecision::Deny { reason } = decision {
            assert_eq!(reason, "forbidden");
        } else {
            panic!("Expected Deny variant");
        }
    }

    #[test]
    fn modify_variant_with_input() {
        let decision = HookDecision::Modify {
            modified_input: Some(json!({"key": "value"})),
            modified_output: None,
        };
        if let HookDecision::Modify { modified_input, modified_output } = decision {
            assert!(modified_input.is_some());
            assert!(modified_output.is_none());
        } else {
            panic!("Expected Modify variant");
        }
    }

    #[test]
    fn modify_variant_with_output() {
        let decision = HookDecision::Modify {
            modified_input: None,
            modified_output: Some(json!({"result": "ok"})),
        };
        if let HookDecision::Modify { modified_input, modified_output } = decision {
            assert!(modified_input.is_none());
            assert!(modified_output.is_some());
        } else {
            panic!("Expected Modify variant");
        }
    }

    #[test]
    fn modify_variant_with_both() {
        let decision = HookDecision::Modify {
            modified_input: Some(json!({"in": 1})),
            modified_output: Some(json!({"out": 2})),
        };
        if let HookDecision::Modify { modified_input, modified_output } = decision {
            assert!(modified_input.is_some());
            assert!(modified_output.is_some());
        } else {
            panic!("Expected Modify variant");
        }
    }

    #[test]
    fn allow_equality() {
        assert_eq!(HookDecision::Allow, HookDecision::Allow);
    }

    #[test]
    fn deny_equality_same_reason() {
        let a = HookDecision::Deny {
            reason: "nope".to_string(),
        };
        let b = HookDecision::Deny {
            reason: "nope".to_string(),
        };
        assert_eq!(a, b);
    }

    #[test]
    fn deny_inequality_different_reason() {
        let a = HookDecision::Deny {
            reason: "nope".to_string(),
        };
        let b = HookDecision::Deny {
            reason: "yep".to_string(),
        };
        assert_ne!(a, b);
    }

    // -- Serde round-trip -----------------------------------------------------

    #[test]
    fn serde_round_trip_allow() {
        let json = serde_json::to_string(&HookDecision::Allow).unwrap();
        let parsed: HookDecision = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, HookDecision::Allow);
    }

    #[test]
    fn serde_round_trip_deny() {
        let decision = HookDecision::Deny {
            reason: "blocked".to_string(),
        };
        let json = serde_json::to_string(&decision).unwrap();
        let parsed: HookDecision = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, decision);
    }

    #[test]
    fn serde_round_trip_modify() {
        let decision = HookDecision::Modify {
            modified_input: Some(json!({"cmd": "safe"})),
            modified_output: None,
        };
        let json = serde_json::to_string(&decision).unwrap();
        let parsed: HookDecision = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, decision);
    }

    #[test]
    fn serde_modify_skips_none_fields() {
        let decision = HookDecision::Modify {
            modified_input: None,
            modified_output: None,
        };
        let json = serde_json::to_string(&decision).unwrap();
        assert!(!json.contains("modified_input"));
        assert!(!json.contains("modified_output"));
    }
}

// ============================================================================
// HookResult Tests
// ============================================================================

mod hook_result_tests {
    use super::*;

    // -- Helper to construct a HookResult with a given decision ---------------

    fn make_result(decision: HookDecision) -> HookResult {
        HookResult {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
            decision,
            command: "test-cmd".to_string(),
        }
    }

    // -- parse_decision: empty / plain text → Allow ---------------------------

    #[test]
    fn parse_decision_empty_string() {
        assert_eq!(HookResult::parse_decision(""), HookDecision::Allow);
    }

    #[test]
    fn parse_decision_whitespace_only() {
        assert_eq!(HookResult::parse_decision("   "), HookDecision::Allow);
    }

    #[test]
    fn parse_decision_plain_text() {
        assert_eq!(
            HookResult::parse_decision("Just some output\nMore output"),
            HookDecision::Allow
        );
    }

    #[test]
    fn parse_decision_newline_only() {
        assert_eq!(HookResult::parse_decision("\n\n"), HookDecision::Allow);
    }

    // -- parse_decision: JSON with decision: "deny" ---------------------------

    #[test]
    fn parse_decision_deny_with_reason() {
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
    fn parse_decision_deny_without_reason_uses_default() {
        let stdout = r#"{"decision": "deny"}"#;
        let decision = HookResult::parse_decision(stdout);
        assert_eq!(
            decision,
            HookDecision::Deny {
                reason: "Hook denied operation".to_string()
            }
        );
    }

    #[test]
    fn parse_decision_deny_with_null_reason_uses_default() {
        let stdout = r#"{"decision": "deny", "reason": null}"#;
        let decision = HookResult::parse_decision(stdout);
        assert_eq!(
            decision,
            HookDecision::Deny {
                reason: "Hook denied operation".to_string()
            }
        );
    }

    // -- parse_decision: JSON with decision: "modify" -------------------------

    #[test]
    fn parse_decision_modify_with_input() {
        let stdout = r#"{"decision": "modify", "modified_input": {"command": "echo hello"}}"#;
        let decision = HookResult::parse_decision(stdout);
        assert_eq!(
            decision,
            HookDecision::Modify {
                modified_input: Some(json!({"command": "echo hello"})),
                modified_output: None,
            }
        );
    }

    #[test]
    fn parse_decision_modify_with_output() {
        let stdout = r#"{"decision": "modify", "modified_output": {"result": "ok"}}"#;
        let decision = HookResult::parse_decision(stdout);
        assert_eq!(
            decision,
            HookDecision::Modify {
                modified_input: None,
                modified_output: Some(json!({"result": "ok"})),
            }
        );
    }

    #[test]
    fn parse_decision_modify_with_both() {
        let stdout = r#"{"decision": "modify", "modified_input": {"a": 1}, "modified_output": {"b": 2}}"#;
        let decision = HookResult::parse_decision(stdout);
        assert_eq!(
            decision,
            HookDecision::Modify {
                modified_input: Some(json!({"a": 1})),
                modified_output: Some(json!({"b": 2})),
            }
        );
    }

    #[test]
    fn parse_decision_modify_without_fields() {
        let stdout = r#"{"decision": "modify"}"#;
        let decision = HookResult::parse_decision(stdout);
        assert_eq!(
            decision,
            HookDecision::Modify {
                modified_input: None,
                modified_output: None,
            }
        );
    }

    // -- parse_decision: JSON with unknown decision → Allow -------------------

    #[test]
    fn parse_decision_unknown_decision_string() {
        let stdout = r#"{"decision": "unknown"}"#;
        assert_eq!(HookResult::parse_decision(stdout), HookDecision::Allow);
    }

    #[test]
    fn parse_decision_empty_decision_string() {
        let stdout = r#"{"decision": ""}"#;
        assert_eq!(HookResult::parse_decision(stdout), HookDecision::Allow);
    }

    // -- parse_decision: invalid JSON → Allow ---------------------------------

    #[test]
    fn parse_decision_invalid_json() {
        assert_eq!(
            HookResult::parse_decision("{invalid json"),
            HookDecision::Allow
        );
    }

    #[test]
    fn parse_decision_json_without_decision_field() {
        let stdout = r#"{"foo": "bar"}"#;
        assert_eq!(HookResult::parse_decision(stdout), HookDecision::Allow);
    }

    #[test]
    fn parse_decision_json_array() {
        // Arrays don't have a "decision" field (not an object)
        assert_eq!(HookResult::parse_decision("[1,2,3]"), HookDecision::Allow);
    }

    // -- parse_decision: first-line extraction --------------------------------

    #[test]
    fn parse_decision_uses_first_line_only() {
        let stdout = "{\"decision\": \"deny\", \"reason\": \"blocked\"}\nSome debug output\nMore lines";
        let decision = HookResult::parse_decision(stdout);
        assert_eq!(
            decision,
            HookDecision::Deny {
                reason: "blocked".to_string()
            }
        );
    }

    #[test]
    fn parse_decision_ignores_json_on_second_line() {
        let stdout = "Some prefix\n{\"decision\": \"deny\"}";
        // First line is not JSON starting with '{', so Allow
        assert_eq!(HookResult::parse_decision(stdout), HookDecision::Allow);
    }

    // -- is_denied() ----------------------------------------------------------

    #[test]
    fn is_denied_true_for_deny() {
        let result = make_result(HookDecision::Deny {
            reason: "nope".to_string(),
        });
        assert!(result.is_denied());
    }

    #[test]
    fn is_denied_false_for_allow() {
        let result = make_result(HookDecision::Allow);
        assert!(!result.is_denied());
    }

    #[test]
    fn is_denied_false_for_modify() {
        let result = make_result(HookDecision::Modify {
            modified_input: Some(json!({})),
            modified_output: None,
        });
        assert!(!result.is_denied());
    }

    // -- has_modifications() --------------------------------------------------

    #[test]
    fn has_modifications_true_for_modify() {
        let result = make_result(HookDecision::Modify {
            modified_input: None,
            modified_output: None,
        });
        assert!(result.has_modifications());
    }

    #[test]
    fn has_modifications_false_for_allow() {
        let result = make_result(HookDecision::Allow);
        assert!(!result.has_modifications());
    }

    #[test]
    fn has_modifications_false_for_deny() {
        let result = make_result(HookDecision::Deny {
            reason: "nope".to_string(),
        });
        assert!(!result.has_modifications());
    }
}

// ============================================================================
// HookDef Tests
// ============================================================================

mod hook_def_tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn new_sets_command_and_defaults() {
        let hook = HookDef::new("echo hello");
        assert_eq!(hook.command, "echo hello");
        assert_eq!(hook.timeout, 30);
        assert!(hook.blocking);
    }

    #[test]
    fn new_accepts_string() {
        let cmd: String = "echo world".to_string();
        let hook = HookDef::new(cmd);
        assert_eq!(hook.command, "echo world");
    }

    #[test]
    fn with_timeout_overrides_default() {
        let hook = HookDef::new("cmd").with_timeout(10);
        assert_eq!(hook.timeout, 10);
        // blocking still at default
        assert!(hook.blocking);
    }

    #[test]
    fn with_blocking_false() {
        let hook = HookDef::new("cmd").with_blocking(false);
        assert!(!hook.blocking);
        // timeout still at default
        assert_eq!(hook.timeout, 30);
    }

    #[test]
    fn builder_chaining() {
        let hook = HookDef::new("my-command")
            .with_timeout(5)
            .with_blocking(false);
        assert_eq!(hook.command, "my-command");
        assert_eq!(hook.timeout, 5);
        assert!(!hook.blocking);
    }

    #[test]
    fn timeout_duration_default() {
        let hook = HookDef::new("cmd");
        assert_eq!(hook.timeout_duration(), Duration::from_secs(30));
    }

    #[test]
    fn timeout_duration_custom() {
        let hook = HookDef::new("cmd").with_timeout(120);
        assert_eq!(hook.timeout_duration(), Duration::from_secs(120));
    }

    #[test]
    fn timeout_duration_zero() {
        let hook = HookDef::new("cmd").with_timeout(0);
        assert_eq!(hook.timeout_duration(), Duration::from_secs(0));
    }

    // -- Serde deserialization defaults ---------------------------------------

    #[test]
    fn serde_minimal_json_uses_defaults() {
        let json = r#"{"command": "echo test"}"#;
        let hook: HookDef = serde_json::from_str(json).unwrap();
        assert_eq!(hook.command, "echo test");
        assert_eq!(hook.timeout, 30);
        assert!(hook.blocking);
    }

    #[test]
    fn serde_full_json_overrides_defaults() {
        let json = r#"{"command": "echo test", "timeout": 15, "blocking": false}"#;
        let hook: HookDef = serde_json::from_str(json).unwrap();
        assert_eq!(hook.timeout, 15);
        assert!(!hook.blocking);
    }

    #[test]
    fn serde_round_trip() {
        let hook = HookDef::new("cmd").with_timeout(10).with_blocking(false);
        let json = serde_json::to_string(&hook).unwrap();
        let parsed: HookDef = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.command, hook.command);
        assert_eq!(parsed.timeout, hook.timeout);
        assert_eq!(parsed.blocking, hook.blocking);
    }
}

// ============================================================================
// HookConfig Tests
// ============================================================================

mod hook_config_tests {
    use super::*;

    #[test]
    fn new_creates_empty_hooks() {
        let config = HookConfig::new("Bash");
        assert_eq!(config.matcher, "Bash");
        assert!(config.hooks.is_empty());
    }

    #[test]
    fn new_accepts_string() {
        let m: String = "Read".to_string();
        let config = HookConfig::new(m);
        assert_eq!(config.matcher, "Read");
    }

    #[test]
    fn with_hook_adds_hooks() {
        let config = HookConfig::new("Bash")
            .with_hook(HookDef::new("echo pre"))
            .with_hook(HookDef::new("echo post").with_blocking(false));

        assert_eq!(config.hooks.len(), 2);
        assert_eq!(config.hooks[0].command, "echo pre");
        assert_eq!(config.hooks[1].command, "echo post");
        assert!(config.hooks[0].blocking);
        assert!(!config.hooks[1].blocking);
    }

    // -- matches: wildcard ----------------------------------------------------

    #[test]
    fn matches_wildcard_matches_everything() {
        let config = HookConfig::new("*");
        assert!(config.matches("Bash"));
        assert!(config.matches("Read"));
        assert!(config.matches(""));
        assert!(config.matches("anything at all"));
    }

    // -- matches: exact string ------------------------------------------------

    #[test]
    fn matches_exact_string() {
        let config = HookConfig::new("Bash");
        assert!(config.matches("Bash"));
        assert!(!config.matches("bash")); // case-sensitive
        assert!(!config.matches("BashTool"));
        assert!(!config.matches("Read"));
    }

    // -- matches: regex patterns ----------------------------------------------

    #[test]
    fn matches_regex_star() {
        let config = HookConfig::new("Bash.*");
        assert!(config.matches("Bash"));
        assert!(config.matches("BashTool"));
        assert!(!config.matches("Read"));
    }

    #[test]
    fn matches_regex_character_class() {
        let config = HookConfig::new("Ba(sh|z)");
        assert!(config.matches("Bash"));
        assert!(config.matches("Baz"));
        assert!(!config.matches("BashExtra")); // anchored
    }

    #[test]
    fn matches_regex_dot() {
        let config = HookConfig::new("B.s.");
        assert!(config.matches("Bash"));
        assert!(config.matches("Base"));
        assert!(!config.matches("BashTool")); // anchored
    }

    // -- matches: fallback substring ------------------------------------------

    #[test]
    fn matches_invalid_regex_falls_back_to_substring() {
        // "[invalid" is not a valid regex, so substring matching kicks in
        let config = HookConfig::new("[invalid");
        assert!(config.matches("[invalid"));
        assert!(config.matches("some[invalid]thing"));
        assert!(!config.matches("something-else"));
    }

    #[test]
    fn matches_empty_matcher_no_substring_match() {
        let config = HookConfig::new("");
        // Empty string is not a wildcard, not equal to "Bash",
        // but "" is valid regex (matches empty string) so it anchors to "^$"
        // which only matches "". "Bash" doesn't match "^$".
        assert!(config.matches(""));
        assert!(!config.matches("Bash"));
    }

    // -- Serde round-trip -----------------------------------------------------

    #[test]
    fn serde_round_trip() {
        let config = HookConfig::new("Bash")
            .with_hook(HookDef::new("echo test").with_timeout(5).with_blocking(false));

        let json = serde_json::to_string(&config).unwrap();
        let parsed: HookConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.matcher, "Bash");
        assert_eq!(parsed.hooks.len(), 1);
        assert_eq!(parsed.hooks[0].command, "echo test");
        assert_eq!(parsed.hooks[0].timeout, 5);
        assert!(!parsed.hooks[0].blocking);
    }
}

// ============================================================================
// HooksFile Tests
// ============================================================================

mod hooks_file_tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn new_is_empty() {
        let file = HooksFile::new();
        assert!(file.hooks.is_empty());
    }

    #[test]
    fn default_is_same_as_new() {
        let file = HooksFile::default();
        assert!(file.hooks.is_empty());
    }

    // -- from_json: valid JSON ------------------------------------------------

    #[test]
    fn from_json_valid_full_config() {
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
        // Defaults applied when omitted
        assert_eq!(session[0].hooks[0].timeout, 30);
        assert!(session[0].hooks[0].blocking);
    }

    #[test]
    fn from_json_empty_hooks_object() {
        let json = r#"{"hooks": {}}"#;
        let file = HooksFile::from_json(json).unwrap();
        assert!(file.hooks.is_empty());
    }

    #[test]
    fn from_json_multiple_configs_per_event() {
        let json = r#"{
            "hooks": {
                "PreToolUse": [
                    {"matcher": "Bash", "hooks": [{"command": "echo 1"}]},
                    {"matcher": "Read", "hooks": [{"command": "echo 2"}]}
                ]
            }
        }"#;

        let file = HooksFile::from_json(json).unwrap();
        let pre = file.hooks.get("PreToolUse").unwrap();
        assert_eq!(pre.len(), 2);
        assert_eq!(pre[0].matcher, "Bash");
        assert_eq!(pre[1].matcher, "Read");
    }

    // -- from_json: invalid JSON ----------------------------------------------

    #[test]
    fn from_json_invalid_syntax() {
        assert!(HooksFile::from_json("not json").is_err());
    }

    #[test]
    fn from_json_valid_json_wrong_structure() {
        // Valid JSON but not the expected structure - serde may accept or reject
        // depending on the shape. An array is not a valid HooksFile.
        let result = HooksFile::from_json("[]");
        assert!(result.is_err());
    }

    #[test]
    fn from_json_empty_string() {
        assert!(HooksFile::from_json("").is_err());
    }

    // -- to_json round-trip ---------------------------------------------------

    #[test]
    fn to_json_round_trip_empty() {
        let file = HooksFile::new();
        let json = file.to_json().unwrap();
        let parsed: HooksFile = HooksFile::from_json(&json).unwrap();
        assert!(parsed.hooks.is_empty());
    }

    #[test]
    fn to_json_round_trip_with_data() {
        let mut file = HooksFile::new();
        file.hooks.insert(
            "PreToolUse".to_string(),
            vec![HookConfig::new("Bash").with_hook(HookDef::new("echo hi"))],
        );
        file.hooks.insert(
            "SessionStart".to_string(),
            vec![HookConfig::new("*").with_hook(HookDef::new("echo start"))],
        );

        let json = file.to_json().unwrap();
        let parsed = HooksFile::from_json(&json).unwrap();
        assert_eq!(parsed.hooks.len(), 2);

        let pre = parsed.hooks.get("PreToolUse").unwrap();
        assert_eq!(pre[0].matcher, "Bash");
        assert_eq!(pre[0].hooks[0].command, "echo hi");

        let start = parsed.hooks.get("SessionStart").unwrap();
        assert_eq!(start[0].matcher, "*");
    }

    // -- get_for_event --------------------------------------------------------

    #[test]
    fn get_for_event_returns_matching_configs() {
        let mut file = HooksFile::new();
        file.hooks.insert(
            "PreToolUse".to_string(),
            vec![HookConfig::new("Bash"), HookConfig::new("Read")],
        );

        let configs = file.get_for_event(&HookEventType::PreToolUse);
        assert_eq!(configs.len(), 2);
    }

    #[test]
    fn get_for_event_returns_empty_when_no_match() {
        let mut file = HooksFile::new();
        file.hooks.insert(
            "PreToolUse".to_string(),
            vec![HookConfig::new("Bash")],
        );

        let configs = file.get_for_event(&HookEventType::SessionStart);
        assert!(configs.is_empty());
    }

    #[test]
    fn get_for_event_empty_file() {
        let file = HooksFile::new();
        let configs = file.get_for_event(&HookEventType::PreToolUse);
        assert!(configs.is_empty());
    }

    #[test]
    fn get_for_event_all_event_types() {
        let mut file = HooksFile::new();
        file.hooks
            .insert("PreToolUse".to_string(), vec![HookConfig::new("*")]);
        file.hooks
            .insert("PostToolUse".to_string(), vec![HookConfig::new("*")]);
        file.hooks
            .insert("SessionStart".to_string(), vec![HookConfig::new("*")]);
        file.hooks
            .insert("SessionEnd".to_string(), vec![HookConfig::new("*")]);
        file.hooks
            .insert("Notification".to_string(), vec![HookConfig::new("*")]);
        file.hooks
            .insert("UserPromptSubmit".to_string(), vec![HookConfig::new("*")]);

        assert_eq!(
            file.get_for_event(&HookEventType::PreToolUse).len(),
            1
        );
        assert_eq!(
            file.get_for_event(&HookEventType::PostToolUse).len(),
            1
        );
        assert_eq!(
            file.get_for_event(&HookEventType::SessionStart).len(),
            1
        );
        assert_eq!(
            file.get_for_event(&HookEventType::SessionEnd).len(),
            1
        );
        assert_eq!(
            file.get_for_event(&HookEventType::Notification).len(),
            1
        );
        assert_eq!(
            file.get_for_event(&HookEventType::UserPromptSubmit).len(),
            1
        );
    }

    // -- merge ----------------------------------------------------------------

    #[test]
    fn merge_combines_same_event_type() {
        let mut file1 = HooksFile::new();
        file1.hooks.insert(
            "PreToolUse".to_string(),
            vec![HookConfig::new("Bash")],
        );

        let mut file2 = HooksFile::new();
        file2.hooks.insert(
            "PreToolUse".to_string(),
            vec![HookConfig::new("Read")],
        );

        file1.merge(file2);

        let pre = file1.hooks.get("PreToolUse").unwrap();
        assert_eq!(pre.len(), 2);
        assert_eq!(pre[0].matcher, "Bash");
        assert_eq!(pre[1].matcher, "Read");
    }

    #[test]
    fn merge_adds_new_event_type() {
        let mut file1 = HooksFile::new();
        file1.hooks.insert(
            "PreToolUse".to_string(),
            vec![HookConfig::new("Bash")],
        );

        let mut file2 = HooksFile::new();
        file2.hooks.insert(
            "SessionStart".to_string(),
            vec![HookConfig::new("*")],
        );

        file1.merge(file2);

        assert_eq!(file1.hooks.len(), 2);
        assert!(file1.hooks.contains_key("PreToolUse"));
        assert!(file1.hooks.contains_key("SessionStart"));
    }

    #[test]
    fn merge_empty_into_populated() {
        let mut file1 = HooksFile::new();
        file1.hooks.insert(
            "PreToolUse".to_string(),
            vec![HookConfig::new("Bash")],
        );

        let file2 = HooksFile::new();
        file1.merge(file2);

        assert_eq!(file1.hooks.len(), 1);
        let pre = file1.hooks.get("PreToolUse").unwrap();
        assert_eq!(pre.len(), 1);
    }

    #[test]
    fn merge_populated_into_empty() {
        let mut file1 = HooksFile::new();

        let mut file2 = HooksFile::new();
        file2.hooks.insert(
            "PreToolUse".to_string(),
            vec![HookConfig::new("Bash")],
        );

        file1.merge(file2);

        assert_eq!(file1.hooks.len(), 1);
    }

    #[test]
    fn merge_multiple_hooks_same_event() {
        let mut file1 = HooksFile::new();
        file1.hooks.insert(
            "PreToolUse".to_string(),
            vec![HookConfig::new("Bash"), HookConfig::new("Read")],
        );

        let mut file2 = HooksFile::new();
        file2.hooks.insert(
            "PreToolUse".to_string(),
            vec![HookConfig::new("Write")],
        );

        file1.merge(file2);

        let pre = file1.hooks.get("PreToolUse").unwrap();
        assert_eq!(pre.len(), 3);
        assert_eq!(pre[0].matcher, "Bash");
        assert_eq!(pre[1].matcher, "Read");
        assert_eq!(pre[2].matcher, "Write");
    }

    // -- load_from_file -------------------------------------------------------

    #[test]
    fn load_from_file_nonexistent_returns_empty() {
        let file = HooksFile::load_from_file(Path::new("/nonexistent/path/hooks.json"));
        assert!(file.is_ok());
        assert!(file.unwrap().hooks.is_empty());
    }

    #[test]
    fn load_from_file_valid_json() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("hooks.json");
        let json = r#"{"hooks": {"PreToolUse": [{"matcher": "Bash", "hooks": [{"command": "echo hi"}]}]}}"#;
        std::fs::write(&path, json).unwrap();

        let file = HooksFile::load_from_file(&path).unwrap();
        let pre = file.hooks.get("PreToolUse").unwrap();
        assert_eq!(pre[0].matcher, "Bash");
    }

    #[test]
    fn load_from_file_invalid_json_returns_error() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("hooks.json");
        std::fs::write(&path, "not json").unwrap();

        let result = HooksFile::load_from_file(&path);
        assert!(result.is_err());
    }
}

// ============================================================================
// HookManager Tests
// ============================================================================

mod hook_manager_tests {
    use super::*;
    use std::path::Path;

    // -- Construction ---------------------------------------------------------

    #[test]
    fn new_creates_with_default_paths() {
        let manager = HookManager::new();
        // User config should be under ~/.shannon/hooks.json
        assert!(manager
            .user_config_path()
            .to_str()
            .unwrap()
            .ends_with("hooks.json"));
        // Project config should be .shannon/hooks.json
        assert_eq!(
            manager.project_config_path().to_str().unwrap(),
            ".shannon/hooks.json"
        );
    }

    #[test]
    fn with_paths_custom_paths() {
        let user = PathBuf::from("/custom/user/hooks.json");
        let project = PathBuf::from("/custom/project/hooks.json");

        let manager = HookManager::with_paths(user.clone(), project.clone());

        assert_eq!(manager.user_config_path(), user);
        assert_eq!(manager.project_config_path(), project);
    }

    #[test]
    fn default_is_same_as_new() {
        let mgr = HookManager::default();
        assert!(mgr.user_config_path().ends_with("hooks.json"));
    }

    // -- hooks_file() returns default when unloaded ---------------------------

    #[test]
    fn hooks_file_default_is_empty() {
        let manager = HookManager::with_paths(
            PathBuf::from("/nonexistent"),
            PathBuf::from("/nonexistent"),
        );
        assert!(manager.hooks_file().hooks.is_empty());
    }

    // -- load() from non-existent files ---------------------------------------

    #[test]
    fn load_nonexistent_files_no_error() {
        let mut manager = HookManager::with_paths(
            PathBuf::from("/nonexistent/user_hooks.json"),
            PathBuf::from("/nonexistent/project_hooks.json"),
        );

        let result = manager.load();
        assert!(result.is_ok());
        assert!(manager.hooks_file().hooks.is_empty());
        assert!(manager.configured_event_types().is_empty());
    }

    // -- load() from real files -----------------------------------------------

    #[test]
    fn load_user_config_only() {
        let temp_dir = tempfile::tempdir().unwrap();
        let user_path = temp_dir.path().join("user_hooks.json");
        let project_path = temp_dir.path().join("project_hooks.json");

        let user_json = r#"{
            "hooks": {
                "PreToolUse": [
                    {"matcher": "Bash", "hooks": [{"command": "echo user"}]}
                ]
            }
        }"#;
        std::fs::write(&user_path, user_json).unwrap();

        let mut manager = HookManager::with_paths(user_path, project_path);
        manager.load().unwrap();

        let event_types = manager.configured_event_types();
        assert_eq!(event_types.len(), 1);
        assert_eq!(event_types[0], HookEventType::PreToolUse);
    }

    #[test]
    fn load_project_config_only() {
        let temp_dir = tempfile::tempdir().unwrap();
        let user_path = temp_dir.path().join("user_hooks.json");
        let project_path = temp_dir.path().join("project_hooks.json");

        let project_json = r#"{
            "hooks": {
                "SessionStart": [
                    {"matcher": "*", "hooks": [{"command": "echo start"}]}
                ]
            }
        }"#;
        std::fs::write(&project_path, project_json).unwrap();

        let mut manager = HookManager::with_paths(user_path, project_path);
        manager.load().unwrap();

        let event_types = manager.configured_event_types();
        assert_eq!(event_types.len(), 1);
        assert_eq!(event_types[0], HookEventType::SessionStart);
    }

    #[test]
    fn load_merges_user_and_project() {
        let temp_dir = tempfile::tempdir().unwrap();
        let user_path = temp_dir.path().join("user_hooks.json");
        let project_path = temp_dir.path().join("project_hooks.json");

        let user_json = r#"{
            "hooks": {
                "PreToolUse": [
                    {"matcher": "Bash", "hooks": [{"command": "echo user"}]}
                ]
            }
        }"#;

        let project_json = r#"{
            "hooks": {
                "PreToolUse": [
                    {"matcher": "Read", "hooks": [{"command": "echo project"}]}
                ]
            }
        }"#;

        std::fs::write(&user_path, user_json).unwrap();
        std::fs::write(&project_path, project_json).unwrap();

        let mut manager = HookManager::with_paths(user_path, project_path);
        manager.load().unwrap();

        let configs = manager.hooks_file().get_for_event(&HookEventType::PreToolUse);
        assert_eq!(configs.len(), 2);
    }

    // -- load_from_path -------------------------------------------------------

    #[test]
    fn load_from_path_valid_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("custom_hooks.json");

        let json = r#"{
            "hooks": {
                "Notification": [
                    {"matcher": "*", "hooks": [{"command": "notify-handler"}]}
                ]
            }
        }"#;
        std::fs::write(&path, json).unwrap();

        let mut manager = HookManager::with_paths(
            PathBuf::from("/nonexistent"),
            PathBuf::from("/nonexistent"),
        );
        manager.load_from_path(&path).unwrap();

        let event_types = manager.configured_event_types();
        assert_eq!(event_types.len(), 1);
        assert_eq!(event_types[0], HookEventType::Notification);
    }

    #[test]
    fn load_from_path_invalid_json_errors() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("bad_hooks.json");
        std::fs::write(&path, "not valid json").unwrap();

        let mut manager = HookManager::with_paths(
            PathBuf::from("/nonexistent"),
            PathBuf::from("/nonexistent"),
        );
        assert!(manager.load_from_path(&path).is_err());
    }

    #[test]
    fn load_from_path_nonexistent_file_returns_empty() {
        let mut manager = HookManager::with_paths(
            PathBuf::from("/nonexistent"),
            PathBuf::from("/nonexistent"),
        );
        // load_from_path delegates to load_from_file which returns empty for missing files
        let result = manager.load_from_path(Path::new("/nonexistent/hooks.json"));
        assert!(result.is_ok());
        assert!(manager.hooks_file().hooks.is_empty());
    }

    // -- configured_event_types -----------------------------------------------

    #[test]
    fn configured_event_types_empty_when_no_config() {
        let manager = HookManager::with_paths(
            PathBuf::from("/nonexistent"),
            PathBuf::from("/nonexistent"),
        );
        assert!(manager.configured_event_types().is_empty());
    }

    #[test]
    fn configured_event_types_returns_all_configured() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("hooks.json");

        let json = r#"{
            "hooks": {
                "PreToolUse": [{"matcher": "*", "hooks": [{"command": "a"}]}],
                "SessionStart": [{"matcher": "*", "hooks": [{"command": "b"}]}],
                "Notification": [{"matcher": "*", "hooks": [{"command": "c"}]}]
            }
        }"#;
        std::fs::write(&path, json).unwrap();

        let mut manager = HookManager::with_paths(
            PathBuf::from("/nonexistent"),
            PathBuf::from("/nonexistent"),
        );
        manager.load_from_path(&path).unwrap();

        let mut types: Vec<String> = manager
            .configured_event_types()
            .into_iter()
            .map(|t| t.to_string())
            .collect();
        types.sort();

        assert_eq!(types, vec!["Notification", "PreToolUse", "SessionStart"]);
    }

    #[test]
    fn configured_event_types_ignores_unknown_keys() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("hooks.json");

        // Include an unknown event type key alongside a known one
        let json = r#"{
            "hooks": {
                "PreToolUse": [{"matcher": "*", "hooks": [{"command": "a"}]}],
                "UnknownEvent": [{"matcher": "*", "hooks": [{"command": "b"}]}]
            }
        }"#;
        std::fs::write(&path, json).unwrap();

        let mut manager = HookManager::with_paths(
            PathBuf::from("/nonexistent"),
            PathBuf::from("/nonexistent"),
        );
        manager.load_from_path(&path).unwrap();

        let types = manager.configured_event_types();
        // Only the known event type should appear
        assert_eq!(types.len(), 1);
        assert_eq!(types[0], HookEventType::PreToolUse);
    }
}

// ============================================================================
// resolve_results Tests
// ============================================================================

mod resolve_results_tests {
    use super::*;

    fn make_result(decision: HookDecision, cmd: &str) -> HookResult {
        HookResult {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
            decision,
            command: cmd.to_string(),
        }
    }

    #[test]
    fn empty_results_returns_allow() {
        let results: Vec<HookResult> = vec![];
        assert_eq!(HookManager::resolve_results(&results), HookDecision::Allow);
    }

    #[test]
    fn single_allow_returns_allow() {
        let results = vec![make_result(HookDecision::Allow, "cmd")];
        assert_eq!(HookManager::resolve_results(&results), HookDecision::Allow);
    }

    #[test]
    fn multiple_allows_returns_allow() {
        let results = vec![
            make_result(HookDecision::Allow, "cmd1"),
            make_result(HookDecision::Allow, "cmd2"),
            make_result(HookDecision::Allow, "cmd3"),
        ];
        assert_eq!(HookManager::resolve_results(&results), HookDecision::Allow);
    }

    #[test]
    fn single_deny_returns_deny() {
        let results = vec![make_result(
            HookDecision::Deny {
                reason: "forbidden".to_string(),
            },
            "cmd",
        )];
        assert_eq!(
            HookManager::resolve_results(&results),
            HookDecision::Deny {
                reason: "forbidden".to_string()
            }
        );
    }

    #[test]
    fn first_deny_wins_over_later_denys() {
        let results = vec![
            make_result(HookDecision::Allow, "cmd1"),
            make_result(
                HookDecision::Deny {
                    reason: "first".to_string(),
                },
                "cmd2",
            ),
            make_result(
                HookDecision::Deny {
                    reason: "second".to_string(),
                },
                "cmd3",
            ),
        ];
        assert_eq!(
            HookManager::resolve_results(&results),
            HookDecision::Deny {
                reason: "first".to_string()
            }
        );
    }

    #[test]
    fn deny_overrides_modify() {
        let results = vec![
            make_result(
                HookDecision::Modify {
                    modified_input: Some(json!({"v": 1})),
                    modified_output: None,
                },
                "cmd1",
            ),
            make_result(
                HookDecision::Deny {
                    reason: "blocked".to_string(),
                },
                "cmd2",
            ),
        ];
        assert_eq!(
            HookManager::resolve_results(&results),
            HookDecision::Deny {
                reason: "blocked".to_string()
            }
        );
    }

    #[test]
    fn deny_before_modify_stops_immediately() {
        let results = vec![
            make_result(
                HookDecision::Deny {
                    reason: "nope".to_string(),
                },
                "cmd1",
            ),
            make_result(
                HookDecision::Modify {
                    modified_input: Some(json!({"v": 99})),
                    modified_output: None,
                },
                "cmd2",
            ),
        ];
        assert_eq!(
            HookManager::resolve_results(&results),
            HookDecision::Deny {
                reason: "nope".to_string()
            }
        );
    }

    #[test]
    fn last_modify_wins_among_modifies() {
        let results = vec![
            make_result(
                HookDecision::Modify {
                    modified_input: Some(json!({"v": 1})),
                    modified_output: None,
                },
                "cmd1",
            ),
            make_result(
                HookDecision::Modify {
                    modified_input: Some(json!({"v": 2})),
                    modified_output: None,
                },
                "cmd2",
            ),
        ];
        assert_eq!(
            HookManager::resolve_results(&results),
            HookDecision::Modify {
                modified_input: Some(json!({"v": 2})),
                modified_output: None,
            }
        );
    }

    #[test]
    fn modify_with_output_preserved() {
        let results = vec![make_result(
            HookDecision::Modify {
                modified_input: None,
                modified_output: Some(json!({"result": "ok"})),
            },
            "cmd",
        )];
        assert_eq!(
            HookManager::resolve_results(&results),
            HookDecision::Modify {
                modified_input: None,
                modified_output: Some(json!({"result": "ok"})),
            }
        );
    }

    #[test]
    fn allow_does_not_override_modify() {
        let results = vec![
            make_result(
                HookDecision::Modify {
                    modified_input: Some(json!({"x": 1})),
                    modified_output: None,
                },
                "cmd1",
            ),
            make_result(HookDecision::Allow, "cmd2"),
        ];
        assert_eq!(
            HookManager::resolve_results(&results),
            HookDecision::Modify {
                modified_input: Some(json!({"x": 1})),
                modified_output: None,
            }
        );
    }

    #[test]
    fn allow_before_modify_lets_modify_win() {
        let results = vec![
            make_result(HookDecision::Allow, "cmd1"),
            make_result(
                HookDecision::Modify {
                    modified_input: Some(json!({"a": 1})),
                    modified_output: None,
                },
                "cmd2",
            ),
        ];
        assert_eq!(
            HookManager::resolve_results(&results),
            HookDecision::Modify {
                modified_input: Some(json!({"a": 1})),
                modified_output: None,
            }
        );
    }
}

// ============================================================================
// HookError Tests
// ============================================================================

mod hook_error_tests {
    use super::*;

    #[test]
    fn io_error_display() {
        let err = HookError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "file not found",
        ));
        assert!(err.to_string().contains("file not found"));
    }

    #[test]
    fn json_error_display() {
        let err = HookError::Json(serde_json::from_str::<serde_json::Value>("bad").unwrap_err());
        assert!(err.to_string().contains("JSON"));
    }

    #[test]
    fn timeout_error_display() {
        let err = HookError::Timeout {
            command: "sleep 100".to_string(),
            timeout_secs: 5,
        };
        let msg = err.to_string();
        assert!(msg.contains("timed out"));
        assert!(msg.contains("5"));
        assert!(msg.contains("sleep 100"));
    }

    #[test]
    fn command_failed_error_display() {
        let err = HookError::CommandFailed {
            command: "bad-cmd".to_string(),
            exit_code: 127,
            stderr: "command not found".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("127"));
        assert!(msg.contains("bad-cmd"));
    }

    #[test]
    fn invalid_matcher_error_display() {
        let err = HookError::InvalidMatcher("[".to_string());
        assert!(err.to_string().contains("["));
    }

    #[test]
    fn denied_error_display() {
        let err = HookError::Denied {
            reason: "policy violation".to_string(),
        };
        assert!(err.to_string().contains("policy violation"));
    }

    #[test]
    fn home_not_found_error_display() {
        let err = HookError::HomeNotFound;
        assert!(err.to_string().contains("Home"));
    }
}
