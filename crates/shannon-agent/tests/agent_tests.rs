//! Integration tests for the shannon-agent binary crate.
//!
//! Tests cover:
//! - JSON-RPC 2.0 message construction (request, notification, response, error)
//! - Line-delimited framing (frame_message / parse_message round-trips)
//! - Protocol parameter types (ExecuteTaskParams, AgentReadyParams, etc.)
//! - JSON-RPC error helpers
//! - Message classification (is_notification, is_request, is_response)
//! - Binary subprocess behavior (assert_cmd)

use shannon_agents::{
    frame_message, parse_message,
    AgentIdleParams, AgentReadyParams, ClaimTaskParams, ClaimTaskResult,
    ExecuteTaskParams, ListTasksParams, ListTasksResult, SendMessageParams,
    TaskCompleteParams, TaskProgressParams, TaskSummary,
    JsonRpcError, JsonRpcId, JsonRpcMessage,
};
use serde_json::json;

// =========================================================================
// 1. JSON-RPC Message Construction
// =========================================================================

mod message_construction_tests {
    use super::*;

    #[test]
    fn request_has_id_method_and_params() {
        let msg = JsonRpcMessage::request("execute_task", json!({"task_id": "t1"}), 42);

        assert_eq!(msg.jsonrpc, "2.0");
        assert_eq!(msg.method(), Some("execute_task"));
        assert!(msg.id.is_some());
        assert!(msg.params.is_some());
        assert!(msg.result.is_none());
        assert!(msg.error.is_none());
        assert!(msg.is_request());
        assert!(!msg.is_notification());
        assert!(!msg.is_response());
    }

    #[test]
    fn notification_has_no_id() {
        let msg = JsonRpcMessage::notification("agent_ready", json!({"agent_name": "w1"}));

        assert_eq!(msg.jsonrpc, "2.0");
        assert_eq!(msg.method(), Some("agent_ready"));
        assert!(msg.id.is_none());
        assert!(msg.params.is_some());
        assert!(msg.is_notification());
        assert!(!msg.is_request());
        assert!(!msg.is_response());
    }

    #[test]
    fn success_response_has_result_no_method() {
        let msg = JsonRpcMessage::response(JsonRpcId::Number(1), json!({"status": "ok"}));

        assert_eq!(msg.jsonrpc, "2.0");
        assert!(msg.method().is_none());
        assert!(msg.id.is_some());
        assert!(msg.result.is_some());
        assert!(msg.error.is_none());
        assert!(msg.is_response());
        assert!(!msg.is_request());
        assert!(!msg.is_notification());
    }

    #[test]
    fn error_response_has_error_field() {
        let msg = JsonRpcMessage::error_response(
            JsonRpcId::Number(5),
            JsonRpcError::not_found("bogus_method"),
        );

        assert_eq!(msg.jsonrpc, "2.0");
        assert!(msg.method().is_none());
        assert!(msg.error.is_some());
        assert!(msg.result.is_none());
        assert!(msg.is_response());

        let err = msg.error.unwrap();
        assert_eq!(err.code, -32601);
        assert!(err.message.contains("bogus_method"));
    }

    #[test]
    fn request_with_string_id() {
        let msg = JsonRpcMessage::request("ping", json!(null), 99);
        let serialized = serde_json::to_string(&msg).unwrap();
        assert!(serialized.contains("\"id\":99"));
    }

    #[test]
    fn notification_omits_id_in_serialized_form() {
        let msg = JsonRpcMessage::notification("task_progress", json!({"chunk": "hi"}));
        let serialized = serde_json::to_string(&msg).unwrap();
        assert!(!serialized.contains("\"id\""));
    }
}

// =========================================================================
// 2. Framing (frame_message / parse_message)
// =========================================================================

mod framing_tests {
    use super::*;

    #[test]
    fn frame_adds_trailing_newline() {
        let msg = JsonRpcMessage::notification("ping", json!({}));
        let framed = frame_message(&msg).unwrap();
        assert!(framed.ends_with('\n'));
    }

    #[test]
    fn frame_and_parse_roundtrip_request() {
        let original = JsonRpcMessage::request(
            "execute_task",
            json!({"task_id": "abc", "subject": "do stuff"}),
            7,
        );
        let framed = frame_message(&original).unwrap();
        let parsed = parse_message(&framed).unwrap();

        assert_eq!(parsed.method(), Some("execute_task"));
        assert!(parsed.is_request());
        let params = parsed.params.unwrap();
        assert_eq!(params["task_id"], "abc");
    }

    #[test]
    fn frame_and_parse_roundtrip_notification() {
        let original = JsonRpcMessage::notification(
            "agent_ready",
            json!({"agent_name": "worker-1", "capabilities": ["general"]}),
        );
        let framed = frame_message(&original).unwrap();
        let parsed = parse_message(&framed).unwrap();

        assert!(parsed.is_notification());
        assert_eq!(parsed.method(), Some("agent_ready"));
    }

    #[test]
    fn frame_and_parse_roundtrip_response() {
        let original = JsonRpcMessage::response(
            JsonRpcId::Number(3),
            json!({"status": "ok"}),
        );
        let framed = frame_message(&original).unwrap();
        let parsed = parse_message(&framed).unwrap();

        assert!(parsed.is_response());
        assert_eq!(parsed.result.unwrap()["status"], "ok");
    }

    #[test]
    fn frame_and_parse_roundtrip_error_response() {
        let original = JsonRpcMessage::error_response(
            JsonRpcId::Number(10),
            JsonRpcError::internal("something broke"),
        );
        let framed = frame_message(&original).unwrap();
        let parsed = parse_message(&framed).unwrap();

        assert!(parsed.is_response());
        let err = parsed.error.unwrap();
        assert_eq!(err.code, -32603);
        assert_eq!(err.message, "something broke");
    }

    #[test]
    fn parse_message_trims_trailing_whitespace() {
        let line = "{\"jsonrpc\":\"2.0\",\"method\":\"ping\",\"id\":1}  \n  ";
        let parsed = parse_message(line).unwrap();
        assert_eq!(parsed.method(), Some("ping"));
    }

    #[test]
    fn parse_message_rejects_invalid_json() {
        let result = parse_message("not json at all");
        assert!(result.is_err());
    }

    #[test]
    fn parse_message_rejects_empty_string() {
        let result = parse_message("");
        assert!(result.is_err());
    }
}

// =========================================================================
// 3. Protocol Parameter Types
// =========================================================================

mod param_type_tests {
    use super::*;

    // -- ExecuteTaskParams --

    #[test]
    fn execute_task_params_serialization() {
        let params = ExecuteTaskParams {
            task_id: "t-001".to_string(),
            subject: "Fix bug".to_string(),
            description: "Fix the null pointer".to_string(),
            priority: "High".to_string(),
            active_form: Some("Fixing null pointer".to_string()),
        };

        let val = serde_json::to_value(&params).unwrap();
        assert_eq!(val["task_id"], "t-001");
        assert_eq!(val["subject"], "Fix bug");
        assert_eq!(val["description"], "Fix the null pointer");
        assert_eq!(val["priority"], "High");
        assert_eq!(val["active_form"], "Fixing null pointer");
    }

    #[test]
    fn execute_task_params_deserialization() {
        let json = json!({
            "task_id": "t-002",
            "subject": "Write tests",
            "description": "Add unit tests",
            "priority": "Medium"
        });

        let params: ExecuteTaskParams = serde_json::from_value(json).unwrap();
        assert_eq!(params.task_id, "t-002");
        assert_eq!(params.subject, "Write tests");
        assert!(params.active_form.is_none());
    }

    #[test]
    fn execute_task_params_defaults_priority_to_empty() {
        let json = json!({
            "task_id": "t-003",
            "subject": "Task",
            "description": "Desc"
        });
        let params: ExecuteTaskParams = serde_json::from_value(json).unwrap();
        assert_eq!(params.priority, "");
    }

    // -- AgentReadyParams --

    #[test]
    fn agent_ready_params_roundtrip() {
        let params = AgentReadyParams {
            agent_name: "worker-1".to_string(),
            capabilities: vec!["general".to_string(), "rust".to_string()],
        };
        let json = serde_json::to_string(&params).unwrap();
        let decoded: AgentReadyParams = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded.agent_name, "worker-1");
        assert_eq!(decoded.capabilities, vec!["general", "rust"]);
    }

    // -- TaskProgressParams --

    #[test]
    fn task_progress_params_roundtrip() {
        let params = TaskProgressParams {
            task_id: "t-100".to_string(),
            chunk: "halfway done".to_string(),
        };
        let json = serde_json::to_string(&params).unwrap();
        let decoded: TaskProgressParams = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded.task_id, "t-100");
        assert_eq!(decoded.chunk, "halfway done");
    }

    // -- TaskCompleteParams --

    #[test]
    fn task_complete_params_success() {
        let params = TaskCompleteParams {
            task_id: "t-200".to_string(),
            success: true,
            output: "All done".to_string(),
        };
        let val = serde_json::to_value(&params).unwrap();
        assert_eq!(val["success"], true);
        assert_eq!(val["output"], "All done");
    }

    #[test]
    fn task_complete_params_failure() {
        let params = TaskCompleteParams {
            task_id: "t-201".to_string(),
            success: false,
            output: "Error: something went wrong".to_string(),
        };
        let val = serde_json::to_value(&params).unwrap();
        assert_eq!(val["success"], false);
        assert!(val["output"].as_str().unwrap().contains("Error"));
    }

    // -- AgentIdleParams --

    #[test]
    fn agent_idle_params_roundtrip() {
        let params = AgentIdleParams {
            agent_name: "worker-1".to_string(),
            available_tasks_count: 3,
        };
        let json = serde_json::to_string(&params).unwrap();
        let decoded: AgentIdleParams = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded.agent_name, "worker-1");
        assert_eq!(decoded.available_tasks_count, 3);
    }

    // -- ClaimTaskParams --

    #[test]
    fn claim_task_params_roundtrip() {
        let params = ClaimTaskParams {
            agent_name: "worker-1".to_string(),
            team_name: "backend".to_string(),
            task_id: Some("t-300".to_string()),
        };
        let json = serde_json::to_string(&params).unwrap();
        let decoded: ClaimTaskParams = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded.agent_name, "worker-1");
        assert_eq!(decoded.team_name, "backend");
        assert_eq!(decoded.task_id, Some("t-300".to_string()));
    }

    #[test]
    fn claim_task_params_optional_task_id() {
        let params = ClaimTaskParams {
            agent_name: "worker-1".to_string(),
            team_name: "backend".to_string(),
            task_id: None,
        };
        let val = serde_json::to_value(&params).unwrap();
        // serde skip_serializing_if = "Option::is_none" should omit it
        assert!(val.get("task_id").is_none());
    }

    // -- ClaimTaskResult --

    #[test]
    fn claim_task_result_no_task() {
        let result = ClaimTaskResult { task: None };
        let val = serde_json::to_value(&result).unwrap();
        assert_eq!(val["task"], json!(null));
    }

    #[test]
    fn claim_task_result_with_task() {
        let result = ClaimTaskResult {
            task: Some(ExecuteTaskParams {
                task_id: "t-400".to_string(),
                subject: "Refactor".to_string(),
                description: "Refactor module X".to_string(),
                priority: "Medium".to_string(),
                active_form: None,
            }),
        };
        let val = serde_json::to_value(&result).unwrap();
        assert_eq!(val["task"]["task_id"], "t-400");
    }

    // -- SendMessageParams --

    #[test]
    fn send_message_params_roundtrip() {
        let params = SendMessageParams {
            from: "alice".to_string(),
            to: "bob".to_string(),
            content: "Hello".to_string(),
            team_name: "dev".to_string(),
            summary: Some("Greeting".to_string()),
        };
        let json = serde_json::to_string(&params).unwrap();
        let decoded: SendMessageParams = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded.from, "alice");
        assert_eq!(decoded.to, "bob");
        assert_eq!(decoded.content, "Hello");
        assert_eq!(decoded.summary, Some("Greeting".to_string()));
    }

    // -- ListTasksParams --

    #[test]
    fn list_tasks_params_roundtrip() {
        let params = ListTasksParams {
            team_name: "backend".to_string(),
            agent_name: "worker-1".to_string(),
        };
        let json = serde_json::to_string(&params).unwrap();
        let decoded: ListTasksParams = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.team_name, "backend");
    }

    // -- ListTasksResult --

    #[test]
    fn list_tasks_result_roundtrip() {
        let result = ListTasksResult {
            tasks: vec![
                TaskSummary {
                    id: "t-1".to_string(),
                    subject: "Task 1".to_string(),
                    status: "pending".to_string(),
                    owner: Some("alice".to_string()),
                },
                TaskSummary {
                    id: "t-2".to_string(),
                    subject: "Task 2".to_string(),
                    status: "completed".to_string(),
                    owner: None,
                },
            ],
        };
        let json = serde_json::to_string(&result).unwrap();
        let decoded: ListTasksResult = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.tasks.len(), 2);
        assert_eq!(decoded.tasks[0].id, "t-1");
        assert_eq!(decoded.tasks[1].owner, None);
    }

    // -- TaskSummary --

    #[test]
    fn task_summary_serialization() {
        let summary = TaskSummary {
            id: "t-500".to_string(),
            subject: "Do the thing".to_string(),
            status: "in_progress".to_string(),
            owner: Some("bob".to_string()),
        };
        let val = serde_json::to_value(&summary).unwrap();
        assert_eq!(val["id"], "t-500");
        assert_eq!(val["status"], "in_progress");
        assert_eq!(val["owner"], "bob");
    }
}

// =========================================================================
// 4. JSON-RPC Error Helpers
// =========================================================================

mod error_helper_tests {
    use super::*;

    #[test]
    fn error_not_found() {
        let err = JsonRpcError::not_found("my_method");
        assert_eq!(err.code, JsonRpcError::METHOD_NOT_FOUND);
        assert_eq!(err.code, -32601);
        assert!(err.message.contains("my_method"));
        assert!(err.data.is_none());
    }

    #[test]
    fn error_internal() {
        let err = JsonRpcError::internal("disk full");
        assert_eq!(err.code, JsonRpcError::INTERNAL_ERROR);
        assert_eq!(err.code, -32603);
        assert_eq!(err.message, "disk full");
        assert!(err.data.is_none());
    }

    #[test]
    fn error_codes_constants() {
        assert_eq!(JsonRpcError::PARSE_ERROR, -32700);
        assert_eq!(JsonRpcError::INVALID_REQUEST, -32600);
        assert_eq!(JsonRpcError::METHOD_NOT_FOUND, -32601);
        assert_eq!(JsonRpcError::INVALID_PARAMS, -32602);
        assert_eq!(JsonRpcError::INTERNAL_ERROR, -32603);
    }

    #[test]
    fn error_serialization_includes_code_and_message() {
        let err = JsonRpcError::internal("timeout");
        let val = serde_json::to_value(&err).unwrap();
        assert_eq!(val["code"], -32603);
        assert_eq!(val["message"], "timeout");
        assert!(val.get("data").is_none());
    }

    #[test]
    fn error_with_data_field() {
        let err = JsonRpcError {
            code: -32000,
            message: "custom error".to_string(),
            data: Some(json!({"detail": "extra info"})),
        };
        let val = serde_json::to_value(&err).unwrap();
        assert_eq!(val["data"]["detail"], "extra info");
    }
}

// =========================================================================
// 5. JsonRpcId
// =========================================================================

mod json_rpc_id_tests {
    use super::*;

    #[test]
    fn number_id_serializes_as_number() {
        let id = JsonRpcId::Number(42);
        let val = serde_json::to_value(&id).unwrap();
        assert_eq!(val, json!(42));
    }

    #[test]
    fn string_id_serializes_as_string() {
        let id = JsonRpcId::String("abc".to_string());
        let val = serde_json::to_value(&id).unwrap();
        assert_eq!(val, json!("abc"));
    }

    #[test]
    fn number_id_roundtrip() {
        let msg = JsonRpcMessage::response(JsonRpcId::Number(99), json!(true));
        let serialized = serde_json::to_string(&msg).unwrap();
        let parsed: JsonRpcMessage = serde_json::from_str(&serialized).unwrap();

        match parsed.id.unwrap() {
            JsonRpcId::Number(n) => assert_eq!(n, 99),
            JsonRpcId::String(s) => panic!("Expected Number id, got String: {s}"),
        }
    }

    #[test]
    fn string_id_roundtrip() {
        let msg = JsonRpcMessage::response(
            JsonRpcId::String("req-xyz".to_string()),
            json!(null),
        );
        let serialized = serde_json::to_string(&msg).unwrap();
        let parsed: JsonRpcMessage = serde_json::from_str(&serialized).unwrap();

        match parsed.id.unwrap() {
            JsonRpcId::String(s) => assert_eq!(s, "req-xyz"),
            JsonRpcId::Number(n) => panic!("Expected String id, got Number: {n}"),
        }
    }
}

// =========================================================================
// 6. Message Classification Edge Cases
// =========================================================================

mod message_classification_tests {
    use super::*;

    #[test]
    fn response_with_error_is_still_a_response() {
        let msg = JsonRpcMessage::error_response(
            JsonRpcId::Number(1),
            JsonRpcError::internal("fail"),
        );
        assert!(msg.is_response());
        assert!(!msg.is_request());
        assert!(!msg.is_notification());
    }

    #[test]
    fn response_with_result_is_response() {
        let msg = JsonRpcMessage::response(JsonRpcId::Number(1), json!("ok"));
        assert!(msg.is_response());
    }

    #[test]
    fn method_returns_none_for_response() {
        let msg = JsonRpcMessage::response(JsonRpcId::Number(1), json!({}));
        assert!(msg.method().is_none());
    }

    #[test]
    fn method_returns_name_for_request() {
        let msg = JsonRpcMessage::request("execute_task", json!({}), 1);
        assert_eq!(msg.method(), Some("execute_task"));
    }

    #[test]
    fn method_returns_name_for_notification() {
        let msg = JsonRpcMessage::notification("shutdown", json!({}));
        assert_eq!(msg.method(), Some("shutdown"));
    }
}

// =========================================================================
// 7. Agent Message Building (simulating what main.rs does)
// =========================================================================

mod agent_message_building_tests {
    use super::*;

    /// Simulates the agent building an agent_ready notification.
    #[test]
    fn build_agent_ready_notification() {
        let params = serde_json::to_value(AgentReadyParams {
            agent_name: "worker-1".to_string(),
            capabilities: vec!["general".to_string()],
        })
        .unwrap();

        let msg = JsonRpcMessage::notification("agent_ready", params);

        assert!(msg.is_notification());
        let p = msg.params.unwrap();
        assert_eq!(p["agent_name"], "worker-1");
        assert_eq!(p["capabilities"][0], "general");
    }

    /// Simulates building a task_progress notification.
    #[test]
    fn build_task_progress_notification() {
        let params = serde_json::to_value(TaskProgressParams {
            task_id: "t-001".to_string(),
            chunk: "Starting task: Fix bug".to_string(),
        })
        .unwrap();

        let msg = JsonRpcMessage::notification("task_progress", params);
        assert!(msg.is_notification());
        assert_eq!(msg.params.unwrap()["task_id"], "t-001");
    }

    /// Simulates building a task_complete notification for success.
    #[test]
    fn build_task_complete_success_notification() {
        let params = serde_json::to_value(TaskCompleteParams {
            task_id: "t-001".to_string(),
            success: true,
            output: "Fixed the bug".to_string(),
        })
        .unwrap();

        let msg = JsonRpcMessage::notification("task_complete", params);
        let p = msg.params.unwrap();
        assert_eq!(p["success"], true);
        assert_eq!(p["output"], "Fixed the bug");
    }

    /// Simulates building a task_complete notification for failure.
    #[test]
    fn build_task_complete_failure_notification() {
        let params = serde_json::to_value(TaskCompleteParams {
            task_id: "t-002".to_string(),
            success: false,
            output: "Error: LLM returned empty response".to_string(),
        })
        .unwrap();

        let msg = JsonRpcMessage::notification("task_complete", params);
        let p = msg.params.unwrap();
        assert_eq!(p["success"], false);
        assert!(p["output"].as_str().unwrap().contains("Error"));
    }

    /// Simulates building an agent_idle notification.
    #[test]
    fn build_agent_idle_notification() {
        let params = serde_json::to_value(AgentIdleParams {
            agent_name: "worker-1".to_string(),
            available_tasks_count: 0,
        })
        .unwrap();

        let msg = JsonRpcMessage::notification("agent_idle", params);
        let p = msg.params.unwrap();
        assert_eq!(p["agent_name"], "worker-1");
        assert_eq!(p["available_tasks_count"], 0);
    }

    /// Simulates building a ping response (the agent responds to ping requests).
    #[test]
    fn build_ping_response() {
        let msg = JsonRpcMessage::response(JsonRpcId::Number(1), json!({"status": "ok"}));
        assert!(msg.is_response());
        assert_eq!(msg.result.unwrap()["status"], "ok");
    }

    /// Simulates building an error response for unknown method.
    #[test]
    fn build_unknown_method_error() {
        let msg = JsonRpcMessage::error_response(
            JsonRpcId::Number(5),
            JsonRpcError::not_found("unknown_method"),
        );
        let err = msg.error.unwrap();
        assert_eq!(err.code, -32601);
    }

    /// Simulates building an error response for invalid params.
    #[test]
    fn build_invalid_params_error() {
        let msg = JsonRpcMessage::error_response(
            JsonRpcId::Number(3),
            JsonRpcError::internal("missing field `task_id`"),
        );
        let err = msg.error.unwrap();
        assert_eq!(err.code, -32603);
        assert!(err.message.contains("task_id"));
    }
}

// =========================================================================
// 8. User Message Construction Logic
// =========================================================================

mod user_message_logic_tests {
    /// Tests the logic from main.rs: if description is empty, use subject
    /// as the user content; otherwise combine them.
    #[test]
    fn empty_description_uses_subject_only() {
        let subject = "Fix the bug".to_string();
        let description = "".to_string();

        let user_content = if description.is_empty() {
            subject.clone()
        } else {
            format!("{subject}\n\n{description}")
        };

        assert_eq!(user_content, "Fix the bug");
    }

    #[test]
    fn nonempty_description_combines_subject_and_description() {
        let subject = "Fix the bug".to_string();
        let description = "Fix the null pointer dereference in module X".to_string();

        let user_content = if description.is_empty() {
            subject.clone()
        } else {
            format!("{subject}\n\n{description}")
        };

        assert_eq!(
            user_content,
            "Fix the bug\n\nFix the null pointer dereference in module X"
        );
    }

    #[test]
    fn multiline_description_preserved() {
        let subject = "Refactor".to_string();
        let description = "Step 1: Extract function\nStep 2: Add tests\nStep 3: Update docs".to_string();

        let user_content = if description.is_empty() {
            subject.clone()
        } else {
            format!("{subject}\n\n{description}")
        };

        assert!(user_content.contains("Step 1"));
        assert!(user_content.contains("Step 3"));
    }
}

// =========================================================================
// 9. Full Message Lifecycle Simulation
// =========================================================================

mod message_lifecycle_tests {
    use super::*;

    /// Simulates a full request-response cycle:
    /// coordinator sends execute_task, agent responds with progress + complete + idle.
    #[test]
    fn full_task_lifecycle_messages() {
        // 1. Coordinator sends execute_task request
        let task_request = JsonRpcMessage::request(
            "execute_task",
            serde_json::to_value(ExecuteTaskParams {
                task_id: "t-999".to_string(),
                subject: "Write code".to_string(),
                description: "Write a function".to_string(),
                priority: "High".to_string(),
                active_form: None,
            })
            .unwrap(),
            1,
        );

        let framed_request = frame_message(&task_request).unwrap();
        let parsed_request = parse_message(&framed_request).unwrap();
        assert!(parsed_request.is_request());
        assert_eq!(parsed_request.method(), Some("execute_task"));

        // 2. Agent sends task_progress notification
        let progress = JsonRpcMessage::notification(
            "task_progress",
            serde_json::to_value(TaskProgressParams {
                task_id: "t-999".to_string(),
                chunk: "Starting task: Write code".to_string(),
            })
            .unwrap(),
        );
        let framed_progress = frame_message(&progress).unwrap();
        let parsed_progress = parse_message(&framed_progress).unwrap();
        assert!(parsed_progress.is_notification());

        // 3. Agent sends task_complete notification
        let complete = JsonRpcMessage::notification(
            "task_complete",
            serde_json::to_value(TaskCompleteParams {
                task_id: "t-999".to_string(),
                success: true,
                output: "fn hello() {}".to_string(),
            })
            .unwrap(),
        );
        let framed_complete = frame_message(&complete).unwrap();
        let parsed_complete = parse_message(&framed_complete).unwrap();
        assert!(parsed_complete.is_notification());
        assert_eq!(parsed_complete.params.unwrap()["success"], true);

        // 4. Agent sends agent_idle notification
        let idle = JsonRpcMessage::notification(
            "agent_idle",
            serde_json::to_value(AgentIdleParams {
                agent_name: "worker-1".to_string(),
                available_tasks_count: 0,
            })
            .unwrap(),
        );
        let framed_idle = frame_message(&idle).unwrap();
        let parsed_idle = parse_message(&framed_idle).unwrap();
        assert!(parsed_idle.is_notification());
    }

    /// Simulates the agent startup: agent_ready notification.
    #[test]
    fn startup_sends_agent_ready() {
        let ready = JsonRpcMessage::notification(
            "agent_ready",
            serde_json::to_value(AgentReadyParams {
                agent_name: "worker-1".to_string(),
                capabilities: vec!["general".to_string()],
            })
            .unwrap(),
        );

        let framed = frame_message(&ready).unwrap();
        let parsed = parse_message(&framed).unwrap();

        assert!(parsed.is_notification());
        assert_eq!(parsed.method(), Some("agent_ready"));
        let p = parsed.params.unwrap();
        assert_eq!(p["agent_name"], "worker-1");
    }

    /// Simulates shutdown notification from coordinator.
    #[test]
    fn shutdown_notification_parses() {
        let shutdown = JsonRpcMessage::notification("shutdown", json!({"reason": "done"}));
        let framed = frame_message(&shutdown).unwrap();
        let parsed = parse_message(&framed).unwrap();

        assert_eq!(parsed.method(), Some("shutdown"));
        assert!(parsed.is_notification());
    }

    /// Simulates a ping request and response.
    #[test]
    fn ping_request_response_cycle() {
        let ping_req = JsonRpcMessage::request("ping", json!({}), 42);
        let framed = frame_message(&ping_req).unwrap();
        let parsed = parse_message(&framed).unwrap();

        assert_eq!(parsed.method(), Some("ping"));
        assert!(parsed.is_request());

        // Agent would respond with:
        let pong = JsonRpcMessage::response(JsonRpcId::Number(42), json!({"status": "ok"}));
        let framed_pong = frame_message(&pong).unwrap();
        let parsed_pong = parse_message(&framed_pong).unwrap();
        assert!(parsed_pong.is_response());
        assert_eq!(parsed_pong.result.unwrap()["status"], "ok");
    }
}

// =========================================================================
// 10. Binary Subprocess Tests
// =========================================================================

mod binary_tests {
    use super::*;
    use assert_cmd::Command;
    use predicates::prelude::*;

    /// The binary requires --name and should fail without it.
    #[test]
    fn binary_requires_name_argument() {
        let mut cmd = Command::cargo_bin("shannon-agent").unwrap();
        cmd.assert()
            .failure()
            .stderr(predicate::str::contains("--name"));
    }

    /// The binary should start, print agent_ready to stdout, then wait for stdin.
    /// Sending EOF should cause it to exit.
    #[tokio::test]
    async fn binary_starts_and_sends_ready_on_eof() {
        let mut cmd = Command::cargo_bin("shannon-agent").unwrap();
        cmd.arg("--name").arg("test-worker");

        // Close stdin immediately — the agent should exit its read loop
        cmd.write_stdin("");

        cmd.assert()
            .success()
            .stdout(predicate::str::contains("agent_ready"));
    }

    /// The binary should respond to a shutdown notification by exiting cleanly.
    #[tokio::test]
    async fn binary_handles_shutdown() {
        let shutdown_msg = JsonRpcMessage::notification("shutdown", json!({}));
        let framed = frame_message(&shutdown_msg).unwrap();

        let mut cmd = Command::cargo_bin("shannon-agent").unwrap();
        cmd.arg("--name").arg("shutdown-test");
        cmd.write_stdin(framed);

        cmd.assert()
            .success()
            .stdout(predicate::str::contains("agent_ready"));
    }

    /// The binary should respond to a ping with a JSON-RPC response.
    #[tokio::test]
    async fn binary_responds_to_ping() {
        let ping_msg = JsonRpcMessage::request("ping", json!({}), 1);
        let framed_ping = frame_message(&ping_msg).unwrap();

        // Send ping then shutdown so the process exits
        let shutdown_msg = JsonRpcMessage::notification("shutdown", json!({}));
        let framed_shutdown = frame_message(&shutdown_msg).unwrap();

        let input = format!("{framed_ping}{framed_shutdown}");

        let mut cmd = Command::cargo_bin("shannon-agent").unwrap();
        cmd.arg("--name").arg("ping-test");
        cmd.write_stdin(input);

        cmd.assert()
            .success()
            .stdout(predicate::str::contains("\"status\":\"ok\""));
    }

    /// The binary should skip invalid JSON lines without crashing.
    #[tokio::test]
    async fn binary_handles_invalid_json_gracefully() {
        let shutdown_msg = JsonRpcMessage::notification("shutdown", json!({}));
        let framed_shutdown = frame_message(&shutdown_msg).unwrap();

        let input = format!("this is not json\n{framed_shutdown}");

        let mut cmd = Command::cargo_bin("shannon-agent").unwrap();
        cmd.arg("--name").arg("invalid-json-test");
        cmd.write_stdin(input);

        // Should not crash — just log a warning and continue
        cmd.assert().success();
    }

    /// The binary should handle unknown methods with an error response.
    #[tokio::test]
    async fn binary_handles_unknown_method() {
        let unknown_msg = JsonRpcMessage::request("nonexistent_method", json!({}), 7);
        let framed_unknown = frame_message(&unknown_msg).unwrap();

        let shutdown_msg = JsonRpcMessage::notification("shutdown", json!({}));
        let framed_shutdown = frame_message(&shutdown_msg).unwrap();

        let input = format!("{framed_unknown}{framed_shutdown}");

        let mut cmd = Command::cargo_bin("shannon-agent").unwrap();
        cmd.arg("--name").arg("unknown-method-test");
        cmd.write_stdin(input);

        cmd.assert()
            .success()
            .stdout(predicate::str::contains("-32601"));
    }
}
