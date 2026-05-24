//! Integration tests for the shannon-agents crate.
//!
//! Tests cover:
//! - Message types: serialization, construction, factory methods
//! - Coordinator config: defaults, serialization
//! - Multi-agent: agent config, dependency resolution, result types
//! - Sub-agent: lifecycle state transitions
//! - Task types: construction, state transitions, dependency management
//! - Summary types: generation, merging, metrics

use shannon_agents::*;
use std::time::Duration;
use uuid::Uuid;

// =========================================================================
// 1. Message Types
// =========================================================================

mod message_tests {
    use super::*;

    #[test]
    fn message_priority_ordering() {
        assert!(MessagePriority::Critical > MessagePriority::High);
        assert!(MessagePriority::High > MessagePriority::Normal);
        assert!(MessagePriority::Normal > MessagePriority::Low);
        assert!(MessagePriority::Low < MessagePriority::Critical);
    }

    #[test]
    fn message_type_serialization_roundtrip() {
        let types = vec![
            MessageType::Chat,
            MessageType::Protocol,
            MessageType::TaskAssignment,
            MessageType::TaskUpdate,
            MessageType::Error,
            MessageType::Status,
        ];

        for mt in &types {
            let json = serde_json::to_string(mt).expect("serialize MessageType");
            let deserialized: MessageType =
                serde_json::from_str(&json).expect("deserialize MessageType");
            // MessageType doesn't implement PartialEq, so compare via serialization
            assert_eq!(
                serde_json::to_string(mt).unwrap(),
                serde_json::to_string(&deserialized).unwrap()
            );
        }
    }

    #[test]
    fn message_priority_serialization_roundtrip() {
        let priorities = vec![
            MessagePriority::Low,
            MessagePriority::Normal,
            MessagePriority::High,
            MessagePriority::Critical,
        ];

        for p in &priorities {
            let json = serde_json::to_string(p).expect("serialize MessagePriority");
            let deserialized: MessagePriority =
                serde_json::from_str(&json).expect("deserialize MessagePriority");
            assert_eq!(*p, deserialized);
        }
    }

    #[test]
    fn agent_message_new_text() {
        let msg = AgentMessage::new_text(
            "alice".to_string(),
            "bob".to_string(),
            "hello world".to_string(),
        );

        assert_eq!(msg.from, "alice");
        assert_eq!(msg.to, "bob");
        assert_eq!(msg.priority, MessagePriority::Normal);
        assert!(!msg.id.is_nil());
    }

    #[test]
    fn agent_message_broadcast() {
        let msg = AgentMessage::broadcast("alice".to_string(), "team announcement".to_string());

        assert_eq!(msg.from, "alice");
        assert_eq!(msg.to, "*");
        assert_eq!(msg.priority, MessagePriority::Normal);
    }

    #[test]
    fn agent_message_protocol() {
        let protocol_msg = ProtocolMessage::ShutdownRequest {
            reason: "end of session".to_string(),
        };
        let msg = AgentMessage::protocol(
            "coordinator".to_string(),
            "worker-1".to_string(),
            protocol_msg,
        );

        assert_eq!(msg.from, "coordinator");
        assert_eq!(msg.to, "worker-1");
        assert_eq!(msg.priority, MessagePriority::High);
    }

    #[test]
    fn agent_message_serialization_roundtrip() {
        let msg = AgentMessage::new_text(
            "alice".to_string(),
            "bob".to_string(),
            "test content".to_string(),
        );

        let json = serde_json::to_string(&msg).expect("serialize AgentMessage");
        let deserialized: AgentMessage =
            serde_json::from_str(&json).expect("deserialize AgentMessage");

        assert_eq!(msg.id, deserialized.id);
        assert_eq!(msg.from, deserialized.from);
        assert_eq!(msg.to, deserialized.to);
        // MessageType doesn't implement PartialEq; compare via serialization
        assert_eq!(
            serde_json::to_string(&msg.message_type).unwrap(),
            serde_json::to_string(&deserialized.message_type).unwrap()
        );
        assert_eq!(msg.priority, deserialized.priority);
        assert_eq!(msg.timestamp, deserialized.timestamp);
    }

    #[test]
    fn message_content_text_serialization() {
        let content = MessageContent::Text("hello".to_string());
        let json = serde_json::to_string(&content).expect("serialize Text content");
        let deserialized: MessageContent =
            serde_json::from_str(&json).expect("deserialize Text content");
        match deserialized {
            MessageContent::Text(s) => assert_eq!(s, "hello"),
            _ => panic!("Expected Text variant"),
        }
    }

    #[test]
    fn message_content_structured_serialization() {
        let content = MessageContent::Structured(serde_json::json!({"key": "value"}));
        let json = serde_json::to_string(&content).expect("serialize Structured content");
        let deserialized: MessageContent =
            serde_json::from_str(&json).expect("deserialize Structured content");
        match deserialized {
            MessageContent::Structured(val) => assert_eq!(val["key"], "value"),
            _ => panic!("Expected Structured variant"),
        }
    }

    #[test]
    fn message_content_protocol_serialization() {
        let content = MessageContent::Protocol(ProtocolMessage::ShutdownResponse {
            request_id: Uuid::new_v4(),
            approve: true,
            reason: None,
        });
        let json = serde_json::to_string(&content).expect("serialize Protocol content");
        let deserialized: MessageContent =
            serde_json::from_str(&json).expect("deserialize Protocol content");
        match deserialized {
            MessageContent::Protocol(ProtocolMessage::ShutdownResponse { approve, .. }) => {
                assert!(approve);
            }
            _ => panic!("Expected Protocol::ShutdownResponse variant"),
        }
    }

    #[test]
    fn protocol_message_plan_approval_serialization() {
        let msg = ProtocolMessage::PlanApprovalRequest {
            request_id: Uuid::new_v4(),
            plan: "refactor module X".to_string(),
        };
        let json = serde_json::to_string(&msg).expect("serialize PlanApprovalRequest");
        let deserialized: ProtocolMessage =
            serde_json::from_str(&json).expect("deserialize PlanApprovalRequest");

        match deserialized {
            ProtocolMessage::PlanApprovalRequest { plan, .. } => {
                assert_eq!(plan, "refactor module X");
            }
            _ => panic!("Expected PlanApprovalRequest variant"),
        }
    }

    #[test]
    fn broadcast_message_serialization_roundtrip() {
        let msg = AgentMessage::broadcast("coordinator".to_string(), "all hands".to_string());

        let json = serde_json::to_string(&msg).expect("serialize broadcast");
        let deserialized: AgentMessage =
            serde_json::from_str(&json).expect("deserialize broadcast");

        assert_eq!(deserialized.to, "*");
        assert_eq!(deserialized.from, "coordinator");
    }
}

// =========================================================================
// 2. Coordinator
// =========================================================================

mod coordinator_tests {
    use super::*;

    #[test]
    fn coordinator_config_default_values() {
        let config = CoordinatorConfig::default();

        assert_eq!(config.max_team_size, 10);
        assert_eq!(config.message_buffer_size, 100);
        assert!(!config.enable_worktree_isolation);
        assert!(config.worktree_config.is_none());
        assert_eq!(config.heartbeat_interval_secs, 30);
        assert_eq!(config.assignment_strategy, AssignmentStrategy::SelfClaim);
        assert!(!config.delegate_mode);
    }

    #[test]
    fn coordinator_config_serialization_roundtrip() {
        let config = CoordinatorConfig::default();
        let json = serde_json::to_string(&config).expect("serialize CoordinatorConfig");
        let deserialized: CoordinatorConfig =
            serde_json::from_str(&json).expect("deserialize CoordinatorConfig");

        assert_eq!(config.max_team_size, deserialized.max_team_size);
        assert_eq!(config.message_buffer_size, deserialized.message_buffer_size);
        assert_eq!(
            config.enable_worktree_isolation,
            deserialized.enable_worktree_isolation
        );
        assert_eq!(
            config.heartbeat_interval_secs,
            deserialized.heartbeat_interval_secs
        );
        assert_eq!(config.assignment_strategy, deserialized.assignment_strategy);
    }

    #[test]
    fn assignment_strategy_serialization_roundtrip() {
        let strategies = vec![
            AssignmentStrategy::RoundRobin,
            AssignmentStrategy::LeastLoaded,
            AssignmentStrategy::CapabilityBased,
            AssignmentStrategy::FirstAvailable,
            AssignmentStrategy::SelfClaim,
        ];

        for strategy in &strategies {
            let json = serde_json::to_string(strategy).expect("serialize AssignmentStrategy");
            let deserialized: AssignmentStrategy =
                serde_json::from_str(&json).expect("deserialize AssignmentStrategy");
            assert_eq!(*strategy, deserialized);
        }
    }

    #[test]
    fn coordinator_config_custom_values() {
        let config = CoordinatorConfig {
            max_team_size: 5,
            message_buffer_size: 200,
            enable_worktree_isolation: true,
            worktree_config: None,
            heartbeat_interval_secs: 60,
            assignment_strategy: AssignmentStrategy::RoundRobin,
            delegate_mode: false,
            agent_mode: AgentMode::default(),
        };

        assert_eq!(config.max_team_size, 5);
        assert_eq!(config.message_buffer_size, 200);
        assert!(config.enable_worktree_isolation);
        assert_eq!(config.heartbeat_interval_secs, 60);
        assert_eq!(config.assignment_strategy, AssignmentStrategy::RoundRobin);
    }

    #[test]
    fn coordinator_config_custom_serialization() {
        let config = CoordinatorConfig {
            max_team_size: 3,
            message_buffer_size: 50,
            enable_worktree_isolation: false,
            worktree_config: None,
            heartbeat_interval_secs: 15,
            assignment_strategy: AssignmentStrategy::LeastLoaded,
            delegate_mode: false,
            agent_mode: AgentMode::default(),
        };

        let json = serde_json::to_string(&config).expect("serialize custom config");
        let deserialized: CoordinatorConfig =
            serde_json::from_str(&json).expect("deserialize custom config");

        assert_eq!(deserialized.max_team_size, 3);
        assert_eq!(
            deserialized.assignment_strategy,
            AssignmentStrategy::LeastLoaded
        );
        assert_eq!(deserialized.heartbeat_interval_secs, 15);
    }

    #[tokio::test]
    async fn disband_team_removes_team_and_notifies_agents() {
        let config = CoordinatorConfig::default();
        let coordinator = AgentCoordinator::new(config).await.unwrap();

        // Create team with 2 agents
        coordinator
            .create_team("test-team".into(), "A test team".into())
            .await
            .unwrap();
        let cfg1 = TeammateConfig {
            agent_type: "worker".into(),
            ..Default::default()
        };
        let cfg2 = TeammateConfig {
            agent_type: "reviewer".into(),
            ..Default::default()
        };
        coordinator
            .add_teammate("test-team", "agent-1".into(), cfg1)
            .await
            .unwrap();
        coordinator
            .add_teammate("test-team", "agent-2".into(), cfg2)
            .await
            .unwrap();

        // Verify team exists with 2 members
        let status = coordinator.team_status("test-team").await.unwrap();
        assert!(status.contains("2 members"));

        // Disband the team
        coordinator.disband_team("test-team").await.unwrap();

        // Verify team no longer exists
        assert!(coordinator.team_status("test-team").await.is_err());
    }

    #[tokio::test]
    async fn disband_nonexistent_team_returns_error() {
        let config = CoordinatorConfig::default();
        let coordinator = AgentCoordinator::new(config).await.unwrap();

        let result = coordinator.disband_team("no-such-team").await;
        assert!(result.is_err());
    }
}

// =========================================================================
// 3. Multi-Agent
// =========================================================================

mod multi_agent_tests {
    use super::*;

    #[test]
    fn spawn_agent_config_new() {
        let config = SpawnAgentConfig::new("agent-1", "do something");

        assert_eq!(config.name, "agent-1");
        assert_eq!(config.task, "do something");
        assert!(config.model.is_none());
        assert!(config.tools.is_none());
        assert!(config.depends_on.is_empty());
    }

    #[test]
    fn spawn_agent_config_builder() {
        let config = SpawnAgentConfig::new("agent-1", "do something")
            .with_model("claude-sonnet-4-6")
            .with_tools(vec!["read".to_string(), "write".to_string()])
            .depends_on("agent-0");

        assert_eq!(config.model.as_deref(), Some("claude-sonnet-4-6"));
        assert_eq!(config.tools.as_ref().unwrap().len(), 2);
        assert_eq!(config.depends_on.len(), 1);
        assert_eq!(config.depends_on[0], "agent-0");
    }

    #[test]
    fn spawn_agent_config_serialization_roundtrip() {
        let config = SpawnAgentConfig::new("agent-1", "do something")
            .with_model("claude-sonnet-4-6")
            .depends_on("agent-0");

        let json = serde_json::to_string(&config).expect("serialize AgentConfig");
        let deserialized: SpawnAgentConfig =
            serde_json::from_str(&json).expect("deserialize AgentConfig");

        assert_eq!(deserialized.name, "agent-1");
        assert_eq!(deserialized.task, "do something");
        assert_eq!(deserialized.model.as_deref(), Some("claude-sonnet-4-6"));
        assert_eq!(deserialized.depends_on, vec!["agent-0"]);
    }

    #[test]
    fn multi_agent_config_default() {
        let config = MultiAgentConfig::default();

        assert!(config.agents.is_empty());
        assert_eq!(config.max_parallel, 4);
        assert_eq!(config.timeout_secs, 300);
        assert!(!config.fail_fast);
    }

    #[test]
    fn multi_agent_config_builder() {
        let agents = vec![
            SpawnAgentConfig::new("a", "task a"),
            SpawnAgentConfig::new("b", "task b"),
        ];
        let config = MultiAgentConfig::new(agents)
            .with_max_parallel(2)
            .with_timeout(Duration::from_secs(120))
            .with_fail_fast();

        assert_eq!(config.agents.len(), 2);
        assert_eq!(config.max_parallel, 2);
        assert_eq!(config.timeout_secs, 120);
        assert!(config.fail_fast);
    }

    #[test]
    fn multi_agent_config_zero_max_parallel_clamped_to_one() {
        let config = MultiAgentConfig::new(vec![]).with_max_parallel(0);
        assert_eq!(config.max_parallel, 1);
    }

    #[test]
    fn multi_agent_config_serialization_roundtrip() {
        let config = MultiAgentConfig::new(vec![
            SpawnAgentConfig::new("a", "task a").depends_on("b"),
            SpawnAgentConfig::new("b", "task b"),
        ])
        .with_max_parallel(3)
        .with_fail_fast();

        let json = serde_json::to_string(&config).expect("serialize MultiAgentConfig");
        let deserialized: MultiAgentConfig =
            serde_json::from_str(&json).expect("deserialize MultiAgentConfig");

        assert_eq!(deserialized.agents.len(), 2);
        assert_eq!(deserialized.max_parallel, 3);
        assert!(deserialized.fail_fast);
        assert_eq!(deserialized.agents[0].depends_on, vec!["b"]);
    }

    #[test]
    fn agent_result_status_serialization_roundtrip() {
        let statuses = vec![
            AgentResultStatus::Completed,
            AgentResultStatus::Failed,
            AgentResultStatus::Timeout,
            AgentResultStatus::Skipped,
        ];

        for status in &statuses {
            let json = serde_json::to_string(status).expect("serialize AgentResultStatus");
            let deserialized: AgentResultStatus =
                serde_json::from_str(&json).expect("deserialize AgentResultStatus");
            assert_eq!(*status, deserialized);
        }
    }

    #[test]
    fn agent_result_status_display() {
        assert_eq!(AgentResultStatus::Completed.to_string(), "completed");
        assert_eq!(AgentResultStatus::Failed.to_string(), "failed");
        assert_eq!(AgentResultStatus::Timeout.to_string(), "timeout");
        assert_eq!(AgentResultStatus::Skipped.to_string(), "skipped");
    }

    #[test]
    fn agent_result_status_equality() {
        assert_eq!(AgentResultStatus::Completed, AgentResultStatus::Completed);
        assert_ne!(AgentResultStatus::Completed, AgentResultStatus::Failed);
    }

    #[test]
    fn multi_agent_result_all_succeeded() {
        // Empty result set: all_succeeded is true (0 failures, 0 == 0)
        let result = MultiAgentResult {
            agent_results: vec![],
            total_duration: Duration::ZERO,
            success_count: 0,
            failure_count: 0,
        };
        assert!(result.all_succeeded());

        // All succeeded: 3 agents, 3 successes, 0 failures
        let result = MultiAgentResult {
            agent_results: vec![
                MultiAgentTaskResult::completed(
                    "a".to_string(),
                    shannon_core::tools::ToolOutput {
                        content: String::new(),
                        is_error: false,
                        metadata: std::collections::HashMap::new(),
                    },
                    Duration::ZERO,
                ),
                MultiAgentTaskResult::completed(
                    "b".to_string(),
                    shannon_core::tools::ToolOutput {
                        content: String::new(),
                        is_error: false,
                        metadata: std::collections::HashMap::new(),
                    },
                    Duration::ZERO,
                ),
                MultiAgentTaskResult::completed(
                    "c".to_string(),
                    shannon_core::tools::ToolOutput {
                        content: String::new(),
                        is_error: false,
                        metadata: std::collections::HashMap::new(),
                    },
                    Duration::ZERO,
                ),
            ],
            total_duration: Duration::ZERO,
            success_count: 3,
            failure_count: 0,
        };
        assert!(result.all_succeeded());

        // Has failures
        let result = MultiAgentResult {
            agent_results: vec![],
            total_duration: Duration::ZERO,
            success_count: 2,
            failure_count: 1,
        };
        assert!(!result.all_succeeded());
    }

    #[test]
    fn dependency_error_unknown_dependency() {
        let err = DependencyError::UnknownDependency("nonexistent".to_string());
        let display = format!("{err}");
        assert!(display.contains("nonexistent"));
        assert!(display.contains("unknown dependency"));
    }

    #[test]
    fn dependency_error_circular_dependency() {
        let err = DependencyError::CircularDependency(vec![
            "a".to_string(),
            "b".to_string(),
            "a".to_string(),
        ]);
        let display = format!("{err}");
        assert!(display.contains("circular dependency"));
        assert!(display.contains("a -> b -> a"));
    }

    #[test]
    fn dependency_error_duplicate_agent() {
        let err = DependencyError::DuplicateAgent("agent-1".to_string());
        let display = format!("{err}");
        assert!(display.contains("duplicate"));
        assert!(display.contains("agent-1"));
    }

    #[test]
    fn agent_config_dependency_chain() {
        // Test that dependency chains can be constructed via the builder pattern
        let agents = vec![
            SpawnAgentConfig::new("c", "compile").depends_on("b"),
            SpawnAgentConfig::new("a", "analyze"),
            SpawnAgentConfig::new("b", "build").depends_on("a"),
        ];

        // Verify dependency structure
        assert_eq!(agents[0].name, "c");
        assert_eq!(agents[0].depends_on, vec!["b"]);

        assert_eq!(agents[1].name, "a");
        assert!(agents[1].depends_on.is_empty());

        assert_eq!(agents[2].name, "b");
        assert_eq!(agents[2].depends_on, vec!["a"]);
    }

    #[test]
    fn multi_agent_config_with_dependency_chain() {
        let config = MultiAgentConfig::new(vec![
            SpawnAgentConfig::new("deploy", "deploy to prod").depends_on("test"),
            SpawnAgentConfig::new("test", "run tests").depends_on("build"),
            SpawnAgentConfig::new("build", "build project"),
        ]);

        assert_eq!(config.agents.len(), 3);
        assert_eq!(config.agents[0].depends_on, vec!["test"]);
        assert_eq!(config.agents[1].depends_on, vec!["build"]);
        assert!(config.agents[2].depends_on.is_empty());
    }
}

// =========================================================================
// 4. Sub-Agent
// =========================================================================

mod sub_agent_tests {
    use super::*;

    #[test]
    fn agent_config_serde_defaults() {
        // The struct uses #[serde(default = ...)] for model and max_turns.
        // Verify deserialization produces correct defaults when fields are omitted.
        let json = r#"{"name":"test","system_prompt":"do things"}"#;
        let config: shannon_agents::AgentConfig =
            serde_json::from_str(json).expect("deserialize AgentConfig with defaults");

        assert_eq!(config.name, "test");
        assert_eq!(config.model, "claude-sonnet-4-6");
        assert!(config.tools.is_empty());
        assert_eq!(config.max_turns, 50);
        assert!(config.working_directory.as_os_str().is_empty());
        assert!(config.team.is_none());
    }

    #[test]
    fn agent_config_serialization_roundtrip() {
        let config = shannon_agents::AgentConfig {
            name: "worker".to_string(),
            model: "claude-sonnet-4-6".to_string(),
            system_prompt: "You are a helpful worker.".to_string(),
            tools: vec!["read".to_string(), "write".to_string()],
            working_directory: "/tmp/work".into(),
            max_turns: 100,
            team: Some("backend".to_string()),
        };

        let json = serde_json::to_string(&config).expect("serialize AgentConfig");
        let deserialized: shannon_agents::AgentConfig =
            serde_json::from_str(&json).expect("deserialize AgentConfig");

        assert_eq!(deserialized.name, "worker");
        assert_eq!(deserialized.model, "claude-sonnet-4-6");
        assert_eq!(deserialized.system_prompt, "You are a helpful worker.");
        assert_eq!(deserialized.tools.len(), 2);
        assert_eq!(deserialized.max_turns, 100);
        assert_eq!(deserialized.team.as_deref(), Some("backend"));
    }

    #[test]
    fn sub_agent_new_initializes_correctly() {
        let config = shannon_agents::AgentConfig {
            name: "agent-1".to_string(),
            model: Default::default(),
            system_prompt: "test prompt".to_string(),
            tools: Default::default(),
            working_directory: Default::default(),
            max_turns: 10,
            team: None,
        };

        let agent = SubAgent::new(config);

        assert_eq!(agent.name, "agent-1");
        assert_eq!(agent.status, AgentStatus::Spawning);
        assert_eq!(agent.turns_used, 0);
        assert!(agent.last_output.is_none());
        assert!(agent.team.is_none());
        assert!(!agent.id.is_empty());
    }

    #[test]
    fn sub_agent_state_transitions() {
        let config = shannon_agents::AgentConfig {
            name: "agent".to_string(),
            model: Default::default(),
            system_prompt: "test".to_string(),
            tools: Default::default(),
            working_directory: Default::default(),
            max_turns: 10,
            team: None,
        };

        let mut agent = SubAgent::new(config);

        // Spawning -> Idle
        agent.mark_idle();
        assert_eq!(agent.status, AgentStatus::Idle);

        // Idle -> Running
        agent.mark_running();
        assert_eq!(agent.status, AgentStatus::Running);

        // Running -> Completed
        agent.mark_completed();
        assert_eq!(agent.status, AgentStatus::Completed);
    }

    #[test]
    fn sub_agent_failure_transition() {
        let config = shannon_agents::AgentConfig {
            name: "agent".to_string(),
            model: Default::default(),
            system_prompt: "test".to_string(),
            tools: Default::default(),
            working_directory: Default::default(),
            max_turns: 10,
            team: None,
        };

        let mut agent = SubAgent::new(config);
        agent.mark_idle();
        agent.mark_running();
        agent.mark_failed("disk full".to_string());

        assert_eq!(agent.status, AgentStatus::Failed("disk full".to_string()));
    }

    #[test]
    fn sub_agent_turn_tracking() {
        let config = shannon_agents::AgentConfig {
            name: "agent".to_string(),
            model: Default::default(),
            system_prompt: "test".to_string(),
            tools: Default::default(),
            working_directory: Default::default(),
            max_turns: 3,
            team: None,
        };

        let mut agent = SubAgent::new(config);

        assert!(agent.has_turns_remaining());

        agent.record_turn(Some("output 1".to_string()));
        assert_eq!(agent.turns_used, 1);
        assert_eq!(agent.last_output.as_deref(), Some("output 1"));
        assert!(agent.has_turns_remaining());

        agent.record_turn(Some("output 2".to_string()));
        assert_eq!(agent.turns_used, 2);
        assert!(agent.has_turns_remaining());

        // Third turn exhausts max_turns (3), auto-completing
        agent.record_turn(Some("output 3".to_string()));
        assert_eq!(agent.turns_used, 3);
        assert!(!agent.has_turns_remaining());
        assert_eq!(agent.status, AgentStatus::Completed);
    }

    #[test]
    fn sub_agent_serialization_roundtrip() {
        let config = shannon_agents::AgentConfig {
            name: "agent".to_string(),
            model: Default::default(),
            system_prompt: "test".to_string(),
            tools: Default::default(),
            working_directory: Default::default(),
            max_turns: 10,
            team: None,
        };

        let mut agent = SubAgent::new(config);
        agent.mark_idle();

        let json = serde_json::to_string(&agent).expect("serialize SubAgent");
        let deserialized: SubAgent = serde_json::from_str(&json).expect("deserialize SubAgent");

        assert_eq!(agent.id, deserialized.id);
        assert_eq!(agent.name, deserialized.name);
        assert_eq!(agent.status, deserialized.status);
        assert_eq!(agent.turns_used, deserialized.turns_used);
    }

    #[test]
    fn agent_status_serialization_roundtrip() {
        let statuses = vec![
            AgentStatus::Spawning,
            AgentStatus::Idle,
            AgentStatus::Running,
            AgentStatus::Completed,
            AgentStatus::Failed("some error".to_string()),
        ];

        for status in &statuses {
            let json = serde_json::to_string(status).expect("serialize AgentStatus");
            let deserialized: AgentStatus =
                serde_json::from_str(&json).expect("deserialize AgentStatus");
            assert_eq!(*status, deserialized);
        }
    }

    #[test]
    fn agent_status_display() {
        assert_eq!(AgentStatus::Spawning.to_string(), "spawning");
        assert_eq!(AgentStatus::Idle.to_string(), "idle");
        assert_eq!(AgentStatus::Running.to_string(), "running");
        assert_eq!(AgentStatus::Completed.to_string(), "completed");
        assert_eq!(
            AgentStatus::Failed("error msg".to_string()).to_string(),
            "failed: error msg"
        );
    }

    #[test]
    fn agent_status_equality() {
        assert_eq!(AgentStatus::Idle, AgentStatus::Idle);
        assert_eq!(
            AgentStatus::Failed("x".to_string()),
            AgentStatus::Failed("x".to_string())
        );
        assert_ne!(AgentStatus::Idle, AgentStatus::Running);
        assert_ne!(
            AgentStatus::Failed("a".to_string()),
            AgentStatus::Failed("b".to_string())
        );
    }
}

// =========================================================================
// 5. Task Types
// =========================================================================

mod task_tests {
    use super::*;

    #[test]
    fn task_priority_ordering() {
        assert!(TaskPriority::Critical > TaskPriority::High);
        assert!(TaskPriority::High > TaskPriority::Medium);
        assert!(TaskPriority::Medium > TaskPriority::Low);
    }

    #[test]
    fn task_priority_serialization_roundtrip() {
        let priorities = vec![
            TaskPriority::Low,
            TaskPriority::Medium,
            TaskPriority::High,
            TaskPriority::Critical,
        ];

        for p in &priorities {
            let json = serde_json::to_string(p).expect("serialize TaskPriority");
            let deserialized: TaskPriority =
                serde_json::from_str(&json).expect("deserialize TaskPriority");
            assert_eq!(*p, deserialized);
        }
    }

    #[test]
    fn task_status_serialization_roundtrip() {
        let statuses = vec![
            TaskStatus::Pending,
            TaskStatus::InProgress,
            TaskStatus::Completed,
            TaskStatus::Failed("err".to_string()),
            TaskStatus::Blocked,
            TaskStatus::Cancelled,
        ];

        for s in &statuses {
            let json = serde_json::to_string(s).expect("serialize TaskStatus");
            let deserialized: TaskStatus =
                serde_json::from_str(&json).expect("deserialize TaskStatus");
            assert_eq!(*s, deserialized);
        }
    }

    #[test]
    fn task_status_equality() {
        assert_eq!(TaskStatus::Pending, TaskStatus::Pending);
        assert_ne!(TaskStatus::Pending, TaskStatus::InProgress);
        assert_eq!(
            TaskStatus::Failed("x".to_string()),
            TaskStatus::Failed("x".to_string())
        );
        assert_ne!(
            TaskStatus::Failed("a".to_string()),
            TaskStatus::Failed("b".to_string())
        );
    }

    #[test]
    fn dependency_type_serialization_roundtrip() {
        let types = vec![
            DependencyType::MustComplete,
            DependencyType::MustStart,
            DependencyType::ShouldComplete,
        ];

        for dt in &types {
            let json = serde_json::to_string(dt).expect("serialize DependencyType");
            let deserialized: DependencyType =
                serde_json::from_str(&json).expect("deserialize DependencyType");
            assert_eq!(*dt, deserialized);
        }
    }

    #[test]
    fn task_dependency_serialization_roundtrip() {
        let dep = TaskDependency {
            task_id: Uuid::new_v4(),
            depends_on: Uuid::new_v4(),
            dependency_type: DependencyType::MustComplete,
        };

        let json = serde_json::to_string(&dep).expect("serialize TaskDependency");
        let deserialized: TaskDependency =
            serde_json::from_str(&json).expect("deserialize TaskDependency");

        assert_eq!(dep.task_id, deserialized.task_id);
        assert_eq!(dep.depends_on, deserialized.depends_on);
        assert_eq!(dep.dependency_type, deserialized.dependency_type);
    }

    #[test]
    fn agent_task_new() {
        let task = AgentTask::new(
            "Fix the bug".to_string(),
            "Fix the null pointer dereference in module X".to_string(),
            TaskPriority::High,
        );

        assert_eq!(task.subject, "Fix the bug");
        assert_eq!(
            task.description,
            "Fix the null pointer dereference in module X"
        );
        assert_eq!(task.status, TaskStatus::Pending);
        assert_eq!(task.priority, TaskPriority::High);
        assert!(task.owner.is_none());
        assert!(task.blocked_by.is_empty());
        assert!(task.blocks.is_empty());
        assert!(task.active_form.is_none());
        assert!(!task.id.is_nil());
    }

    #[test]
    fn agent_task_is_ready() {
        let mut task = AgentTask::new("Task".to_string(), "desc".to_string(), TaskPriority::Medium);
        assert!(task.is_ready());

        // Adding a dependency makes it not ready
        task.blocked_by.push(Uuid::new_v4());
        assert!(!task.is_ready());

        // Even when status is not Pending, it's not ready
        let mut task2 =
            AgentTask::new("Task".to_string(), "desc".to_string(), TaskPriority::Medium);
        task2.status = TaskStatus::InProgress;
        assert!(!task2.is_ready());
    }

    #[test]
    fn agent_task_is_blocking() {
        let mut task = AgentTask::new("Task".to_string(), "desc".to_string(), TaskPriority::Medium);
        assert!(!task.is_blocking());

        task.blocks.push(Uuid::new_v4());
        assert!(task.is_blocking());
    }

    #[test]
    fn agent_task_add_dependency() {
        let mut task = AgentTask::new("Task".to_string(), "desc".to_string(), TaskPriority::Medium);
        let dep_id = Uuid::new_v4();

        task.add_dependency(dep_id);
        assert_eq!(task.blocked_by.len(), 1);
        assert_eq!(task.blocked_by[0], dep_id);

        // Adding same dependency again is idempotent
        task.add_dependency(dep_id);
        assert_eq!(task.blocked_by.len(), 1);
    }

    #[test]
    fn agent_task_mark_completed() {
        let mut task = AgentTask::new("Task".to_string(), "desc".to_string(), TaskPriority::Medium);
        let before = task.updated_at;

        task.mark_completed();
        assert_eq!(task.status, TaskStatus::Completed);
        assert!(task.updated_at >= before);
    }

    #[test]
    fn agent_task_mark_failed() {
        let mut task = AgentTask::new("Task".to_string(), "desc".to_string(), TaskPriority::Medium);

        task.mark_failed("out of memory".to_string());
        assert_eq!(task.status, TaskStatus::Failed("out of memory".to_string()));
    }

    #[test]
    fn agent_task_assign_to() {
        let mut task = AgentTask::new("Task".to_string(), "desc".to_string(), TaskPriority::Medium);

        task.assign_to("alice".to_string());
        assert_eq!(task.owner.as_deref(), Some("alice"));
        assert_eq!(task.status, TaskStatus::InProgress);
    }

    #[test]
    fn agent_task_serialization_roundtrip() {
        let mut task = AgentTask::new(
            "Fix bug".to_string(),
            "Fix it".to_string(),
            TaskPriority::Critical,
        );
        task.assign_to("bob".to_string());

        let json = serde_json::to_string(&task).expect("serialize AgentTask");
        let deserialized: AgentTask = serde_json::from_str(&json).expect("deserialize AgentTask");

        assert_eq!(task.id, deserialized.id);
        assert_eq!(task.subject, deserialized.subject);
        assert_eq!(task.description, deserialized.description);
        assert_eq!(task.status, deserialized.status);
        assert_eq!(task.priority, deserialized.priority);
        assert_eq!(task.owner, deserialized.owner);
    }
}

// =========================================================================
// 6. Error Types
// =========================================================================

mod error_tests {
    use super::*;

    #[test]
    fn coordination_error_display() {
        let err = CoordinationError::TeamNotFound("backend".to_string());
        assert_eq!(format!("{err}"), "team 'backend' not found");

        let err = CoordinationError::AgentNotFound("alice".to_string());
        assert_eq!(format!("{err}"), "agent 'alice' not found in team");

        let err = CoordinationError::AgentAlreadyMember("alice".to_string(), "backend".to_string());
        assert_eq!(
            format!("{err}"),
            "agent 'alice' is already a member of team 'backend'"
        );

        let err = CoordinationError::MaxTeamSizeExceeded(5);
        assert_eq!(format!("{err}"), "maximum team size (5) exceeded");
    }

    #[test]
    fn task_error_display() {
        let id = Uuid::nil();
        let err = TaskError::TaskNotFound(id);
        let msg = format!("{err}");
        assert!(msg.contains("not found"));

        let err = TaskError::CircularDependency(id);
        let msg = format!("{err}");
        assert!(msg.contains("circular"));

        let err = TaskError::NoAvailableAgents("task-x".to_string());
        let msg = format!("{err}");
        assert!(msg.contains("no available agents"));
    }

    #[test]
    fn agent_error_from_coordination() {
        let coord_err = CoordinationError::TeamNotFound("x".to_string());
        let agent_err: AgentError = coord_err.into();
        let msg = format!("{agent_err}");
        assert!(msg.contains("coordination error"));
    }

    #[test]
    fn agent_error_from_task() {
        let task_err = TaskError::TaskNotFound(Uuid::nil());
        let agent_err: AgentError = task_err.into();
        let msg = format!("{agent_err}");
        assert!(msg.contains("task error"));
    }

    #[test]
    fn agent_error_worktree() {
        let err = AgentError::Worktree("branch already exists".to_string());
        let msg = format!("{err}");
        assert!(msg.contains("worktree error"));
        assert!(msg.contains("branch already exists"));
    }

    #[test]
    fn agent_error_communication() {
        let err = AgentError::Communication("channel closed".to_string());
        let msg = format!("{err}");
        assert!(msg.contains("communication error"));
        assert!(msg.contains("channel closed"));
    }
}

// =========================================================================
// 7. Teammate
// =========================================================================

mod teammate_tests {
    use super::*;

    #[tokio::test]
    async fn teammate_new_initializes_idle() {
        let teammate = Teammate::new("alice".to_string(), TeammateConfig::default());

        assert_eq!(teammate.name, "alice");
        assert!(teammate.is_available().await);
        assert_eq!(teammate.status().await, TeammateStatus::Idle);
    }

    #[tokio::test]
    async fn teammate_config_serialization_roundtrip() {
        let config = TeammateConfig {
            agent_type: "specialist".to_string(),
            capabilities: vec!["rust".to_string(), "testing".to_string()],
            max_concurrent_tasks: 5,
            plan_mode_required: true,
            model: Some("claude-sonnet-4-6".to_string()),
            system_prompt: Some("Be thorough.".to_string()),
            temperature: Some(0.7),
            is_lead: false,
            allowed_tools: vec![],
            permission_mode: None,
            isolation: None,
        };

        let json = serde_json::to_string(&config).expect("serialize TeammateConfig");
        let deserialized: TeammateConfig =
            serde_json::from_str(&json).expect("deserialize TeammateConfig");

        assert_eq!(deserialized.agent_type, "specialist");
        assert_eq!(deserialized.capabilities.len(), 2);
        assert_eq!(deserialized.max_concurrent_tasks, 5);
        assert!(deserialized.plan_mode_required);
        assert_eq!(deserialized.model.as_deref(), Some("claude-sonnet-4-6"));
        assert_eq!(deserialized.temperature, Some(0.7));
    }

    #[tokio::test]
    async fn teammate_has_capability() {
        let config = TeammateConfig {
            capabilities: vec!["rust".to_string(), "testing".to_string()],
            ..Default::default()
        };

        let teammate = Teammate::new("expert".to_string(), config);

        assert!(teammate.has_capability("rust"));
        assert!(teammate.has_capability("RUST")); // case-insensitive
        assert!(teammate.has_capability("testing"));
        assert!(!teammate.has_capability("python"));
    }

    #[tokio::test]
    async fn teammate_state() {
        let teammate = Teammate::new("agent".to_string(), TeammateConfig::default());
        let state = teammate.state().await;

        assert_eq!(state.status, TeammateStatus::Idle);
        assert_eq!(state.active_tasks, 0);
        assert!(state.current_worktree.is_none());
    }

    #[tokio::test]
    async fn teammate_task_lifecycle() {
        let teammate = Teammate::new("worker".to_string(), TeammateConfig::default());
        let task_id = Uuid::new_v4();

        // Assign a task
        let result = teammate.assign_task(task_id).await;
        assert!(result.is_ok());
        assert!(!teammate.is_available().await); // Should be busy now

        // Complete the task
        teammate.complete_task(task_id).await;
        assert!(teammate.is_available().await); // Should be idle again
    }

    #[tokio::test]
    async fn teammate_assign_task_when_not_available() {
        let config = TeammateConfig {
            plan_mode_required: true,
            ..Default::default()
        };
        let teammate = Teammate::new("planner".to_string(), config);

        // Enter plan mode (not available)
        teammate.enter_plan_mode().await.expect("enter plan mode");
        assert!(!teammate.is_available().await);

        let result = teammate.assign_task(Uuid::new_v4()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn teammate_metadata() {
        let teammate = Teammate::new("agent".to_string(), TeammateConfig::default());

        assert!(teammate.get_metadata("key").await.is_none());

        teammate
            .set_metadata("key".to_string(), serde_json::json!("value"))
            .await;
        let val = teammate.get_metadata("key").await;
        assert_eq!(val, Some(serde_json::json!("value")));
    }

    #[tokio::test]
    async fn teammate_created_at() {
        let before = chrono::Utc::now();
        let teammate = Teammate::new("agent".to_string(), TeammateConfig::default());
        let after = chrono::Utc::now();

        let created = teammate.created_at();
        assert!(created >= before);
        assert!(created <= after);
    }

    #[test]
    fn teammate_status_serialization_roundtrip() {
        let statuses = vec![
            TeammateStatus::Idle,
            TeammateStatus::Busy,
            TeammateStatus::Planning,
            TeammateStatus::ShuttingDown,
            TeammateStatus::Stopped,
            TeammateStatus::Error,
        ];

        for status in &statuses {
            let json = serde_json::to_string(status).expect("serialize TeammateStatus");
            let deserialized: TeammateStatus =
                serde_json::from_str(&json).expect("deserialize TeammateStatus");
            assert_eq!(*status, deserialized);
        }
    }

    #[test]
    fn teammate_state_serialization_roundtrip() {
        let state = TeammateState {
            status: TeammateStatus::Busy,
            active_tasks: 3,
            current_worktree: Some("branch-feature-x".to_string()),
            last_activity: chrono::Utc::now(),
        };

        let json = serde_json::to_string(&state).expect("serialize TeammateState");
        let deserialized: TeammateState =
            serde_json::from_str(&json).expect("deserialize TeammateState");

        assert_eq!(state.status, deserialized.status);
        assert_eq!(state.active_tasks, deserialized.active_tasks);
        assert_eq!(state.current_worktree, deserialized.current_worktree);
    }
}

// =========================================================================
// 8. Summary Types
// =========================================================================

mod summary_tests {
    use super::*;
    use std::collections::HashMap;

    fn make_tool_output(
        content: &str,
        is_error: bool,
        tool_name: &str,
    ) -> shannon_core::tools::ToolOutput {
        let mut metadata = HashMap::new();
        metadata.insert("tool_name".into(), serde_json::json!(tool_name));
        shannon_core::tools::ToolOutput {
            content: content.to_string(),
            is_error,
            metadata,
        }
    }

    fn make_simple_output(content: &str, is_error: bool) -> shannon_core::tools::ToolOutput {
        shannon_core::tools::ToolOutput {
            content: content.to_string(),
            is_error,
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn summary_status_serialization_roundtrip() {
        let statuses = vec![
            SummaryStatus::Success,
            SummaryStatus::PartialSuccess,
            SummaryStatus::Failed,
            SummaryStatus::Timeout,
        ];

        for status in &statuses {
            let json = serde_json::to_string(status).expect("serialize SummaryStatus");
            let deserialized: SummaryStatus =
                serde_json::from_str(&json).expect("deserialize SummaryStatus");
            assert_eq!(*status, deserialized);
        }
    }

    #[test]
    fn summary_status_equality() {
        assert_eq!(SummaryStatus::Success, SummaryStatus::Success);
        assert_ne!(SummaryStatus::Success, SummaryStatus::Failed);
        // PartialEq is derived
    }

    #[test]
    fn agent_execution_summary_empty() {
        let summary = AgentExecutionSummary::empty();

        assert!(summary.agent_name.is_empty());
        assert!(summary.is_success());
        assert!(!summary.has_errors());
        assert_eq!(summary.duration_ms, 0);
        assert!(summary.files_modified.is_empty());
        assert!(summary.files_created.is_empty());
        assert!(summary.tools_used.is_empty());
        assert!(summary.errors.is_empty());
    }

    #[test]
    fn agent_execution_summary_serialization_roundtrip() {
        let mut metadata = HashMap::new();
        metadata.insert("env".to_string(), serde_json::json!("test"));

        let summary = AgentExecutionSummary {
            agent_name: "test-agent".to_string(),
            task_description: "analyze code".to_string(),
            status: SummaryStatus::Success,
            duration_ms: 5000,
            files_modified: vec!["src/main.rs".to_string()],
            files_created: vec!["src/new.rs".to_string()],
            tools_used: vec!["read".to_string(), "write".to_string()],
            errors: vec![],
            key_findings: vec!["Found issue".to_string()],
            recommendations: vec!["Fix it".to_string()],
            metadata,
        };

        let json = serde_json::to_string(&summary).expect("serialize AgentExecutionSummary");
        let deserialized: AgentExecutionSummary =
            serde_json::from_str(&json).expect("deserialize AgentExecutionSummary");

        assert_eq!(summary.agent_name, deserialized.agent_name);
        assert_eq!(summary.status, deserialized.status);
        assert_eq!(summary.duration_ms, deserialized.duration_ms);
        assert_eq!(summary.files_modified, deserialized.files_modified);
        assert_eq!(summary.files_created, deserialized.files_created);
        assert_eq!(summary.tools_used, deserialized.tools_used);
        assert_eq!(summary.key_findings, deserialized.key_findings);
    }

    #[test]
    fn success_metrics_empty() {
        let metrics = SuccessMetrics::empty();

        assert_eq!(metrics.total_agents, 0);
        assert_eq!(metrics.success_rate, 0.0);
        assert!(!metrics.all_succeeded());
        assert!(!metrics.has_failures());
    }

    #[test]
    fn success_metrics_serialization_roundtrip() {
        let metrics = SuccessMetrics {
            total_agents: 5,
            successful: 3,
            partial: 1,
            failed: 1,
            timed_out: 0,
            total_files_modified: 10,
            total_files_created: 5,
            total_errors: 3,
            success_rate: 60.0,
        };

        let json = serde_json::to_string(&metrics).expect("serialize SuccessMetrics");
        let deserialized: SuccessMetrics =
            serde_json::from_str(&json).expect("deserialize SuccessMetrics");

        assert_eq!(metrics.total_agents, deserialized.total_agents);
        assert_eq!(metrics.successful, deserialized.successful);
        assert!((metrics.success_rate - deserialized.success_rate).abs() < 0.001);
    }

    #[test]
    fn summarize_with_tool_outputs() {
        let results = vec![
            make_tool_output("read file OK", false, "read"),
            make_tool_output("wrote file", false, "write"),
        ];

        let summary = SummaryGenerator::summarize(&results, "worker-1", "process files");
        assert_eq!(summary.agent_name, "worker-1");
        assert_eq!(summary.task_description, "process files");
        assert_eq!(summary.status, SummaryStatus::Success);
        assert_eq!(summary.tools_used.len(), 2);
        assert!(summary.tools_used.contains(&"read".to_string()));
        assert!(summary.tools_used.contains(&"write".to_string()));
        assert!(summary.errors.is_empty());
    }

    #[test]
    fn summarize_mixed_results() {
        let results = vec![
            make_tool_output("OK", false, "read"),
            make_tool_output("permission denied", true, "write"),
        ];

        let summary = SummaryGenerator::summarize(&results, "agent", "task");
        assert_eq!(summary.status, SummaryStatus::PartialSuccess);
        assert_eq!(summary.errors.len(), 1);
    }

    #[test]
    fn summarize_with_duration() {
        let results = vec![make_simple_output("done", false)];
        let summary = SummaryGenerator::summarize_with_duration(&results, "agent", "task", 2500);
        assert_eq!(summary.duration_ms, 2500);
    }

    #[test]
    fn success_metrics_from_summaries() {
        let summaries = vec![
            AgentExecutionSummary {
                agent_name: "a".to_string(),
                task_description: "t".to_string(),
                status: SummaryStatus::Success,
                duration_ms: 100,
                files_modified: vec!["a.rs".to_string()],
                files_created: vec![],
                tools_used: vec![],
                errors: vec![],
                key_findings: vec![],
                recommendations: vec![],
                metadata: HashMap::new(),
            },
            AgentExecutionSummary {
                agent_name: "b".to_string(),
                task_description: "t".to_string(),
                status: SummaryStatus::Failed,
                duration_ms: 200,
                files_modified: vec![],
                files_created: vec!["b.rs".to_string()],
                tools_used: vec![],
                errors: vec!["err".to_string()],
                key_findings: vec![],
                recommendations: vec![],
                metadata: HashMap::new(),
            },
        ];

        let metrics = SummaryGenerator::success_metrics(&summaries);
        assert_eq!(metrics.total_agents, 2);
        assert_eq!(metrics.successful, 1);
        assert_eq!(metrics.failed, 1);
        assert_eq!(metrics.total_files_modified, 1);
        assert_eq!(metrics.total_files_created, 1);
        assert_eq!(metrics.total_errors, 1);
        assert!((metrics.success_rate - 50.0).abs() < 0.01);
    }
}

// =========================================================================
// 9. Crate-level
// =========================================================================

mod crate_tests {
    #[test]
    fn version_is_set() {
        // VERSION should be a non-empty string
        assert!(!shannon_agents::VERSION.is_empty());
    }

    #[test]
    fn agent_result_type_alias_works() {
        let val: String = "hello".to_string();
        assert_eq!(val, "hello");

        let result: shannon_agents::AgentResult<String> =
            Err(shannon_agents::AgentError::Communication("err".to_string()));
        assert!(result.is_err());
    }
}

// =========================================================================
// InboxSummary tests
// =========================================================================

mod inbox_summary_tests {
    use super::*;

    #[test]
    fn inbox_summary_default() {
        let summary = InboxSummary::default();
        assert_eq!(summary.total, 0);
        assert_eq!(summary.unread, 0);
        assert!(summary.senders.is_empty());
    }

    #[test]
    fn inbox_summary_serialization_roundtrip() {
        let summary = InboxSummary {
            total: 5,
            unread: 2,
            senders: vec!["alice".to_string(), "bob".to_string()],
        };
        let json = serde_json::to_string(&summary).unwrap();
        let decoded: InboxSummary = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.total, 5);
        assert_eq!(decoded.unread, 2);
        assert_eq!(decoded.senders, vec!["alice", "bob"]);
    }
}

// =========================================================================
// Teammate metadata merge tests
// =========================================================================

mod teammate_metadata_tests {
    use super::*;
    use shannon_agents::TeammateConfig;

    fn make_teammate() -> Teammate {
        let config = TeammateConfig {
            agent_type: "worker".to_string(),
            capabilities: vec![],
            max_concurrent_tasks: 3,
            plan_mode_required: false,
            model: None,
            system_prompt: None,
            temperature: None,
            is_lead: false,
            allowed_tools: vec![],
            permission_mode: None,
            isolation: None,
        };
        Teammate::new("test-agent".to_string(), config)
    }

    #[tokio::test]
    async fn merge_metadata_adds_multiple_entries() {
        let agent = make_teammate();
        let entries = std::collections::HashMap::from([
            ("role".to_string(), serde_json::json!("reviewer")),
            ("priority".to_string(), serde_json::json!(42)),
        ]);
        agent.merge_metadata(entries).await;

        assert_eq!(
            agent.get_metadata("role").await,
            Some(serde_json::json!("reviewer"))
        );
        assert_eq!(
            agent.get_metadata("priority").await,
            Some(serde_json::json!(42))
        );
    }

    #[tokio::test]
    async fn merge_metadata_overwrites_existing() {
        let agent = make_teammate();
        agent
            .set_metadata("key".to_string(), serde_json::json!("old"))
            .await;

        let entries =
            std::collections::HashMap::from([("key".to_string(), serde_json::json!("new"))]);
        agent.merge_metadata(entries).await;

        assert_eq!(
            agent.get_metadata("key").await,
            Some(serde_json::json!("new"))
        );
    }

    #[tokio::test]
    async fn merge_metadata_empty_is_noop() {
        let agent = make_teammate();
        agent
            .set_metadata("existing".to_string(), serde_json::json!("value"))
            .await;
        agent.merge_metadata(std::collections::HashMap::new()).await;
        assert_eq!(
            agent.get_metadata("existing").await,
            Some(serde_json::json!("value"))
        );
    }
}

// =========================================================================
// Conversation history tests
// =========================================================================

mod conversation_history_tests {
    use super::*;
    use shannon_agents::{ChatTurn, MockAgentExecutor, Teammate, TeammateConfig};
    use std::sync::Arc;

    fn make_teammate_with_executor() -> Teammate {
        let config = TeammateConfig {
            agent_type: "worker".to_string(),
            capabilities: vec![],
            max_concurrent_tasks: 3,
            plan_mode_required: false,
            model: None,
            system_prompt: Some("You are a test agent.".to_string()),
            temperature: None,
            is_lead: false,
            allowed_tools: vec![],
            permission_mode: None,
            isolation: None,
        };
        let executor = Arc::new(MockAgentExecutor::new("response text"));
        Teammate::with_executor("test-agent".to_string(), config, executor)
    }

    fn make_teammate_no_executor() -> Teammate {
        let config = TeammateConfig {
            agent_type: "worker".to_string(),
            capabilities: vec![],
            max_concurrent_tasks: 3,
            plan_mode_required: false,
            model: None,
            system_prompt: None,
            temperature: None,
            is_lead: false,
            allowed_tools: vec![],
            permission_mode: None,
            isolation: None,
        };
        Teammate::new("test-agent".to_string(), config)
    }

    #[tokio::test]
    async fn history_starts_empty() {
        let agent = make_teammate_no_executor();
        assert!(agent.conversation_history().await.is_empty());
    }

    #[tokio::test]
    async fn clear_history_works() {
        let agent = make_teammate_no_executor();
        agent.clear_history().await;
        assert!(agent.conversation_history().await.is_empty());
    }

    #[tokio::test]
    async fn chat_with_executor_appends_to_history() {
        let agent = make_teammate_with_executor();
        let msg = AgentMessage::new_text(
            "leader".to_string(),
            "test-agent".to_string(),
            "Hello".to_string(),
        );

        let response = agent.handle_chat_message(msg).await.unwrap();
        match &response.content {
            MessageContent::Text(t) => assert_eq!(t, "response text"),
            other => panic!("Expected Text response, got {other:?}"),
        }

        let history = agent.conversation_history().await;
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].role, "user");
        assert_eq!(history[0].content, "Hello");
        assert_eq!(history[1].role, "assistant");
        assert_eq!(history[1].content, "response text");
    }

    #[tokio::test]
    async fn multiple_turns_accumulate_history() {
        let agent = make_teammate_with_executor();

        let msg1 = AgentMessage::new_text(
            "leader".to_string(),
            "test-agent".to_string(),
            "First".to_string(),
        );
        let _ = agent.handle_chat_message(msg1).await.unwrap();

        let msg2 = AgentMessage::new_text(
            "leader".to_string(),
            "test-agent".to_string(),
            "Second".to_string(),
        );
        let _ = agent.handle_chat_message(msg2).await.unwrap();

        let history = agent.conversation_history().await;
        assert_eq!(history.len(), 4); // 2 turns × (user + assistant)
        assert_eq!(history[0].content, "First");
        assert_eq!(history[2].content, "Second");
    }

    #[tokio::test]
    async fn clear_history_resets_context() {
        let agent = make_teammate_with_executor();

        let msg = AgentMessage::new_text(
            "leader".to_string(),
            "test-agent".to_string(),
            "Hello".to_string(),
        );
        let _ = agent.handle_chat_message(msg).await.unwrap();
        assert_eq!(agent.conversation_history().await.len(), 2);

        agent.clear_history().await;
        assert!(agent.conversation_history().await.is_empty());
    }

    #[tokio::test]
    async fn placeholder_mode_does_not_append_history() {
        let agent = make_teammate_no_executor();
        let msg = AgentMessage::new_text(
            "leader".to_string(),
            "test-agent".to_string(),
            "Hello".to_string(),
        );

        let _ = agent.handle_chat_message(msg).await.unwrap();
        // Placeholder mode should not modify history
        assert!(agent.conversation_history().await.is_empty());
    }

    #[test]
    fn chat_turn_serialization() {
        let turn = ChatTurn {
            role: "user".to_string(),
            content: "Hello world".to_string(),
        };
        let json = serde_json::to_string(&turn).unwrap();
        let decoded: ChatTurn = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.role, "user");
        assert_eq!(decoded.content, "Hello world");
    }
}

// =========================================================================
// 6. Sub-Agent Context & Error Handling Tests
// =========================================================================

mod sub_agent_context_tests {
    use super::*;

    #[test]
    fn agent_config_carries_context() {
        let config = AgentConfig {
            name: "reviewer".to_string(),
            model: "claude-sonnet-4".to_string(),
            system_prompt: "You are a code reviewer. Review files for bugs.".to_string(),
            tools: vec!["read".to_string(), "grep".to_string()],
            working_directory: std::env::current_dir().unwrap(),
            max_turns: 10,
            team: None,
        };

        assert_eq!(config.name, "reviewer");
        assert_eq!(config.model, "claude-sonnet-4");
        assert!(config.system_prompt.contains("code reviewer"));
        assert_eq!(config.tools.len(), 2);
        assert_eq!(config.max_turns, 10);
        assert!(config.team.is_none());
    }

    #[test]
    fn agent_config_team_assignment() {
        let config = AgentConfig {
            name: "worker-1".to_string(),
            model: "claude-sonnet-4".to_string(),
            system_prompt: "Fix bugs".to_string(),
            tools: vec!["read".to_string(), "write".to_string()],
            working_directory: std::env::current_dir().unwrap(),
            max_turns: 5,
            team: Some("bugfix-team".to_string()),
        };

        assert_eq!(config.team.as_deref(), Some("bugfix-team"));
    }

    #[test]
    fn spawn_agent_config_includes_task_context() {
        let config = SpawnAgentConfig::new("analyzer", "Analyze src/lib.rs for performance issues")
            .with_model("claude-sonnet-4")
            .with_tools(vec!["read".to_string(), "grep".to_string()]);

        // Task description serves as context for the sub-agent
        assert!(config.task.contains("performance issues"));
        assert_eq!(config.name, "analyzer");
        assert_eq!(config.model.as_deref(), Some("claude-sonnet-4"));
        assert_eq!(config.tools.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn multi_agent_dependency_chain_defines_context_order() {
        // Agent B depends on A — A's output becomes B's context
        let config = MultiAgentConfig::new(vec![
            SpawnAgentConfig::new("reader", "Read all source files")
                .with_tools(vec!["read".to_string(), "glob".to_string()]),
            SpawnAgentConfig::new("analyzer", "Analyze code quality based on reader output")
                .depends_on("reader")
                .with_tools(vec!["read".to_string()]),
            SpawnAgentConfig::new("writer", "Write report based on analysis")
                .depends_on("analyzer")
                .with_tools(vec!["write".to_string()]),
        ]);

        assert_eq!(config.agents.len(), 3);
        assert!(config.agents[0].depends_on.is_empty());
        assert_eq!(config.agents[1].depends_on, vec!["reader"]);
        assert_eq!(config.agents[2].depends_on, vec!["analyzer"]);
    }

    #[test]
    fn multi_agent_result_tracks_individual_failures() {
        use shannon_core::tools::ToolOutput;

        // Mixed results: 2 succeeded, 1 failed
        let result = MultiAgentResult {
            agent_results: vec![
                MultiAgentTaskResult::completed(
                    "a".to_string(),
                    ToolOutput {
                        content: "Success A".to_string(),
                        is_error: false,
                        metadata: std::collections::HashMap::new(),
                    },
                    Duration::ZERO,
                ),
                MultiAgentTaskResult::completed(
                    "b".to_string(),
                    ToolOutput {
                        content: "Error: file not found".to_string(),
                        is_error: true,
                        metadata: std::collections::HashMap::new(),
                    },
                    Duration::ZERO,
                ),
                MultiAgentTaskResult::completed(
                    "c".to_string(),
                    ToolOutput {
                        content: "Success C".to_string(),
                        is_error: false,
                        metadata: std::collections::HashMap::new(),
                    },
                    Duration::ZERO,
                ),
            ],
            total_duration: Duration::from_secs(5),
            success_count: 2,
            failure_count: 1,
        };

        assert!(!result.all_succeeded());
        assert_eq!(result.success_count, 2);
        assert_eq!(result.failure_count, 1);

        // Verify individual error is captured
        let failed = result
            .agent_results
            .iter()
            .find(|r| r.agent_name == "b")
            .unwrap();
        assert!(failed.output.as_ref().unwrap().is_error);
        assert!(failed.output.as_ref().unwrap().content.contains("Error"));
    }

    #[test]
    fn multi_agent_result_all_succeeded() {
        use shannon_core::tools::ToolOutput;

        let result = MultiAgentResult {
            agent_results: vec![
                MultiAgentTaskResult::completed(
                    "a".to_string(),
                    ToolOutput {
                        content: String::new(),
                        is_error: false,
                        metadata: std::collections::HashMap::new(),
                    },
                    Duration::ZERO,
                ),
                MultiAgentTaskResult::completed(
                    "b".to_string(),
                    ToolOutput {
                        content: String::new(),
                        is_error: false,
                        metadata: std::collections::HashMap::new(),
                    },
                    Duration::ZERO,
                ),
            ],
            total_duration: Duration::ZERO,
            success_count: 2,
            failure_count: 0,
        };

        assert!(result.all_succeeded());
    }

    #[test]
    fn agent_error_propagation_via_message() {
        // Sub-agent sends error back to coordinator via AgentMessage
        let mut error_msg = AgentMessage::new_text(
            "worker-1".to_string(),
            "coordinator".to_string(),
            "TASK_FAILED: Permission denied writing to /etc/config".to_string(),
        );
        error_msg.priority = MessagePriority::High;

        assert_eq!(error_msg.from, "worker-1");
        assert_eq!(error_msg.to, "coordinator");
        assert_eq!(error_msg.priority, MessagePriority::High);
        match &error_msg.content {
            MessageContent::Text(t) => assert!(t.contains("TASK_FAILED")),
            _ => panic!("expected text content"),
        }
    }

    #[test]
    fn multi_agent_config_fail_fast_mode() {
        let config = MultiAgentConfig::new(vec![
            SpawnAgentConfig::new("a", "task a"),
            SpawnAgentConfig::new("b", "task b"),
        ])
        .with_fail_fast();

        assert!(config.fail_fast, "fail_fast should stop on first error");
    }

    #[test]
    fn multi_agent_config_timeout_limits_execution() {
        let config = MultiAgentConfig::new(vec![SpawnAgentConfig::new(
            "slow-agent",
            "long running task",
        )])
        .with_timeout(Duration::from_secs(30));

        assert_eq!(
            config.timeout_secs, 30,
            "timeout should limit agent execution time"
        );
    }
}
