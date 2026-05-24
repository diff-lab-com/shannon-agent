//! Integration tests for state management (session save/load/restore).
//!
//! Tests:

#![allow(clippy::field_reassign_with_default)]
//! - Session save/load round-trip with message preservation
//! - Session listing with metadata
//! - Multi-session management
//! - Empty session handling
//! - Session data integrity

#[cfg(test)]
mod state_tests {
    use shannon_core::api::{Message, MessageContent};
    use shannon_core::state::{SessionData, SessionPersistMetadata, StateManager};
    use tempfile::TempDir;
    use uuid::Uuid;

    /// Create a StateManager backed by a temporary directory.
    fn make_manager() -> (StateManager, TempDir) {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let mgr = StateManager::with_sessions_dir(dir.path().to_path_buf())
            .expect("failed to create StateManager");
        (mgr, dir)
    }

    fn sample_messages() -> Vec<Message> {
        vec![
            Message {
                role: "system".to_string(),
                content: MessageContent::Text("You are a helpful assistant.".to_string()),
            },
            Message {
                role: "user".to_string(),
                content: MessageContent::Text("Hello, world!".to_string()),
            },
            Message {
                role: "assistant".to_string(),
                content: MessageContent::Text("Hi! How can I help?".to_string()),
            },
        ]
    }

    fn sample_metadata() -> SessionPersistMetadata {
        SessionPersistMetadata {
            model: "claude-3-5-sonnet".to_string(),
            total_input_tokens: 50,
            total_output_tokens: 25,
            ..Default::default()
        }
    }

    // ── Save/Load round-trip ──────────────────────────────────────────────

    #[test]
    fn test_session_save_load_roundtrip() {
        let (mgr, _dir) = make_manager();
        let session_id = Uuid::new_v4();
        let messages = sample_messages();
        let metadata = sample_metadata();

        mgr.save_session(&session_id, &messages, &metadata)
            .expect("save should succeed");

        let loaded = mgr
            .load_session(&session_id)
            .expect("load should succeed")
            .expect("session should exist");

        assert_eq!(loaded.session_id, session_id);
        assert_eq!(loaded.messages.len(), 3);
        assert_eq!(loaded.metadata.model, "claude-3-5-sonnet");
        assert_eq!(loaded.metadata.total_input_tokens, 50);
        assert_eq!(loaded.metadata.total_output_tokens, 25);
        assert_eq!(loaded.metadata.turn_count, 1); // 1 user message
    }

    #[test]
    fn test_session_messages_preserved() {
        let (mgr, _dir) = make_manager();
        let session_id = Uuid::new_v4();

        let messages = vec![
            Message {
                role: "user".to_string(),
                content: MessageContent::Text("First message".to_string()),
            },
            Message {
                role: "assistant".to_string(),
                content: MessageContent::Text("First response".to_string()),
            },
            Message {
                role: "user".to_string(),
                content: MessageContent::Text("Second message".to_string()),
            },
            Message {
                role: "assistant".to_string(),
                content: MessageContent::Text("Second response".to_string()),
            },
        ];

        mgr.save_session(&session_id, &messages, &SessionPersistMetadata::default())
            .expect("save should succeed");

        let loaded = mgr.load_session(&session_id).unwrap().unwrap();
        assert_eq!(loaded.messages.len(), 4);
        assert_eq!(loaded.messages[0].role, "user");
        assert_eq!(loaded.messages[1].role, "assistant");
        assert_eq!(loaded.messages[2].role, "user");
        assert_eq!(loaded.messages[3].role, "assistant");
        assert_eq!(loaded.metadata.turn_count, 2);
    }

    #[test]
    fn test_session_load_nonexistent() {
        let (mgr, _dir) = make_manager();
        let result = mgr.load_session(&Uuid::new_v4()).expect("should not error");
        assert!(result.is_none(), "nonexistent session should return None");
    }

    #[test]
    fn test_session_metadata_updated_at() {
        let (mgr, _dir) = make_manager();
        let session_id = Uuid::new_v4();
        let mut metadata = SessionPersistMetadata::default();
        metadata.model = "test-model".to_string();

        mgr.save_session(&session_id, &sample_messages(), &metadata)
            .expect("save should succeed");

        let loaded = mgr.load_session(&session_id).unwrap().unwrap();
        // updated_at should have been set to now during save
        assert!(loaded.metadata.updated_at <= chrono::Utc::now());
    }

    // ── Multi-session ────────────────────────────────────────────────────

    #[test]
    fn test_multiple_sessions_independent() {
        let (mgr, _dir) = make_manager();
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();

        let msg1 = vec![Message {
            role: "user".to_string(),
            content: MessageContent::Text("Session 1".to_string()),
        }];
        let msg2 = vec![Message {
            role: "user".to_string(),
            content: MessageContent::Text("Session 2".to_string()),
        }];

        let mut meta1 = SessionPersistMetadata::default();
        meta1.model = "model-1".to_string();
        let mut meta2 = SessionPersistMetadata::default();
        meta2.model = "model-2".to_string();

        mgr.save_session(&id1, &msg1, &meta1).expect("save 1");
        mgr.save_session(&id2, &msg2, &meta2).expect("save 2");

        let loaded1 = mgr.load_session(&id1).unwrap().unwrap();
        let loaded2 = mgr.load_session(&id2).unwrap().unwrap();

        assert_eq!(loaded1.metadata.model, "model-1");
        assert_eq!(loaded2.metadata.model, "model-2");
        assert_ne!(loaded1.session_id, loaded2.session_id);
    }

    // ── Empty session ────────────────────────────────────────────────────

    #[test]
    fn test_empty_session_save_load() {
        let (mgr, _dir) = make_manager();
        let session_id = Uuid::new_v4();

        mgr.save_session(&session_id, &[], &SessionPersistMetadata::default())
            .expect("save empty session");

        let loaded = mgr.load_session(&session_id).unwrap().unwrap();
        assert!(loaded.messages.is_empty());
        assert_eq!(loaded.metadata.turn_count, 0);
    }

    // ── SessionData helpers ──────────────────────────────────────────────

    #[test]
    fn test_session_data_first_user_message_preview() {
        let session = SessionData {
            session_id: Uuid::new_v4(),
            metadata: SessionPersistMetadata::default(),
            messages: vec![
                Message {
                    role: "system".to_string(),
                    content: MessageContent::Text("System prompt".to_string()),
                },
                Message {
                    role: "user".to_string(),
                    content: MessageContent::Text(
                        "This is a long user message that should be truncated".to_string(),
                    ),
                },
            ],
        };

        let preview = session.first_user_message_preview(20).unwrap();
        assert!(preview.len() <= 20);
        assert!(preview.ends_with("..."));
    }

    #[test]
    fn test_session_data_no_user_message_preview() {
        let session = SessionData {
            session_id: Uuid::new_v4(),
            metadata: SessionPersistMetadata::default(),
            messages: vec![Message {
                role: "system".to_string(),
                content: MessageContent::Text("Just a system message".to_string()),
            }],
        };

        assert!(session.first_user_message_preview(50).is_none());
    }

    // ── Session overwrite ────────────────────────────────────────────────

    #[test]
    fn test_session_overwrite() {
        let (mgr, _dir) = make_manager();
        let session_id = Uuid::new_v4();

        let v1 = vec![Message {
            role: "user".to_string(),
            content: MessageContent::Text("Version 1".to_string()),
        }];
        let v2 = vec![
            Message {
                role: "user".to_string(),
                content: MessageContent::Text("Version 2".to_string()),
            },
            Message {
                role: "assistant".to_string(),
                content: MessageContent::Text("Response".to_string()),
            },
        ];

        mgr.save_session(&session_id, &v1, &SessionPersistMetadata::default())
            .expect("save v1");
        mgr.save_session(&session_id, &v2, &SessionPersistMetadata::default())
            .expect("save v2");

        let loaded = mgr.load_session(&session_id).unwrap().unwrap();
        assert_eq!(loaded.messages.len(), 2);
        assert_eq!(loaded.messages[0].role, "user");
    }

    // ── Session listing ──────────────────────────────────────────────────

    #[test]
    fn test_list_sessions() {
        let (mgr, _dir) = make_manager();
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();

        let mut meta = SessionPersistMetadata::default();
        meta.model = "test".to_string();
        meta.title = Some("Test session".to_string());

        mgr.save_session(&id1, &sample_messages(), &meta)
            .expect("save 1");
        mgr.save_session(&id2, &sample_messages(), &meta)
            .expect("save 2");

        let sessions = mgr.list_persisted_sessions().expect("list should succeed");
        assert_eq!(sessions.len(), 2);
    }
}
