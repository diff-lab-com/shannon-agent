//! Integration tests for SessionRecovery: create, append, restore round-trip,
//! crash recovery, multi-session, and metadata persistence.

#[cfg(test)]
mod session_recovery_tests {
    use chrono::Utc;
    use shannon_core::api::{ContentBlock, Message, MessageContent};
    use shannon_core::session_recovery::SessionRecovery;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn make_user_msg(text: &str) -> Message {
        Message {
            role: "user".to_string(),
            content: MessageContent::Text(text.to_string()),
        }
    }

    fn make_assistant_msg(text: &str) -> Message {
        Message {
            role: "assistant".to_string(),
            content: MessageContent::Blocks(vec![ContentBlock::Text {
                text: text.to_string(),
            }]),
        }
    }

    fn text_of(msg: &Message) -> String {
        match &msg.content {
            MessageContent::Text(t) => t.clone(),
            MessageContent::Blocks(blocks) => blocks
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect(),
        }
    }

    // ── Tests ──

    #[test]
    fn test_session_create_append_restore_round_trip() {
        let tmp = TempDir::new().unwrap();
        let recovery = SessionRecovery::with_dir(tmp.path().to_path_buf()).unwrap();
        let project = PathBuf::from("/tmp/test-project");

        // Create session
        let session_id = recovery.create_session(&project, "test-model").unwrap();
        assert!(!session_id.is_empty(), "Session ID should be non-empty");

        // Append messages
        let msg1 = make_user_msg("Hello");
        let msg2 = make_assistant_msg("Hi there!");
        let msg3 = make_user_msg("How are you?");
        let msg4 = make_assistant_msg("I'm fine, thanks!");

        recovery.append_message(&project, &session_id, 0, &msg1).unwrap();
        recovery.append_message(&project, &session_id, 1, &msg2).unwrap();
        recovery.append_message(&project, &session_id, 2, &msg3).unwrap();
        recovery.append_message(&project, &session_id, 3, &msg4).unwrap();

        // Load and verify
        let loaded = recovery.load_messages(&project, &session_id).unwrap();
        assert_eq!(loaded.len(), 4, "Should load all 4 messages");
        assert_eq!(text_of(&loaded[0]), "Hello");
        assert_eq!(text_of(&loaded[1]), "Hi there!");
        assert_eq!(text_of(&loaded[2]), "How are you?");
        assert_eq!(text_of(&loaded[3]), "I'm fine, thanks!");
    }

    #[test]
    fn test_session_append_batch_and_restore() {
        let tmp = TempDir::new().unwrap();
        let recovery = SessionRecovery::with_dir(tmp.path().to_path_buf()).unwrap();
        let project = PathBuf::from("/tmp/batch-test");

        let session_id = recovery.create_session(&project, "test-model").unwrap();

        let messages = vec![
            make_user_msg("Turn 1"),
            make_assistant_msg("Response 1"),
            make_user_msg("Turn 2"),
            make_assistant_msg("Response 2"),
        ];

        recovery.append_messages(&project, &session_id, 0, &messages).unwrap();

        let loaded = recovery.load_messages(&project, &session_id).unwrap();
        assert_eq!(loaded.len(), 4);
        assert_eq!(text_of(&loaded[0]), "Turn 1");
        assert_eq!(text_of(&loaded[3]), "Response 2");
    }

    #[test]
    fn test_session_recovery_after_partial_write() {
        // Simulate crash: write valid entries, then append truncated garbage.
        let tmp = TempDir::new().unwrap();
        let recovery = SessionRecovery::with_dir(tmp.path().to_path_buf()).unwrap();
        let project = PathBuf::from("/tmp/crash-test");

        let session_id = recovery.create_session(&project, "test-model").unwrap();

        // Write 3 valid messages
        recovery.append_message(&project, &session_id, 0, &make_user_msg("msg1")).unwrap();
        recovery.append_message(&project, &session_id, 1, &make_assistant_msg("msg2")).unwrap();
        recovery.append_message(&project, &session_id, 2, &make_user_msg("msg3")).unwrap();

        // Manually append a truncated/partial line to simulate crash
        let project_dir = recovery.project_session_dir(&project);
        let log_path = recovery.session_log_path(&project_dir, &session_id);
        use std::fs::OpenOptions;
        use std::io::Write;
        let mut file = OpenOptions::new().append(true).open(&log_path).unwrap();
        file.write_all(b"{\"seq\":3,\"message\":{\"role\":\"user\",\"content\":\"partial...\n").unwrap();
        file.flush().unwrap();

        // Load: should get the 3 complete entries, skip the partial
        let loaded = recovery.load_messages(&project, &session_id).unwrap();
        assert_eq!(loaded.len(), 3, "Should load 3 complete entries, skip partial");
        assert_eq!(text_of(&loaded[2]), "msg3");
    }

    #[test]
    fn test_session_list_and_latest() {
        let tmp = TempDir::new().unwrap();
        let recovery = SessionRecovery::with_dir(tmp.path().to_path_buf()).unwrap();
        let project = PathBuf::from("/tmp/list-test");

        // Create 3 sessions
        let _id1 = recovery.create_session(&project, "model-a").unwrap();
        let _id2 = recovery.create_session(&project, "model-b").unwrap();
        let id3 = recovery.create_session(&project, "model-c").unwrap();

        // List all
        let sessions = recovery.list_sessions(&project).unwrap();
        assert_eq!(sessions.len(), 3, "Should list 3 sessions");

        // Latest should be id3 (most recently created)
        let latest = recovery.get_latest_session(&project).unwrap();
        assert!(latest.is_some(), "Should find latest session");
        let meta = latest.unwrap();
        assert_eq!(meta.id, id3);

        // Verify metadata fields
        assert_eq!(meta.model, "model-c");
        assert_eq!(meta.message_count, 0);
    }

    #[test]
    fn test_session_metadata_persistence() {
        let tmp = TempDir::new().unwrap();
        let recovery = SessionRecovery::with_dir(tmp.path().to_path_buf()).unwrap();
        let project = PathBuf::from("/tmp/meta-test");

        let session_id = recovery.create_session(&project, "test-model").unwrap();

        // Add messages to update metadata
        recovery.append_message(&project, &session_id, 0, &make_user_msg("hello")).unwrap();
        recovery.append_message(&project, &session_id, 1, &make_assistant_msg("world")).unwrap();

        // Load metadata
        let meta = recovery.load_metadata(&project, &session_id).unwrap();
        assert_eq!(meta.id, session_id);
        assert_eq!(meta.model, "test-model");
        assert_eq!(meta.message_count, 2, "Message count should reflect appended messages");
        assert!(meta.created_at <= meta.updated_at, "updated_at should be >= created_at");

        // Save custom metadata and reload
        let custom = shannon_core::session_recovery::RecoveryMetadata {
            id: session_id.clone(),
            project_path: project.clone(),
            created_at: meta.created_at,
            updated_at: Utc::now(),
            message_count: 99,
            model: "custom-model".to_string(),
        };
        recovery.save_metadata(&project, &custom).unwrap();

        let reloaded = recovery.load_metadata(&project, &session_id).unwrap();
        assert_eq!(reloaded.model, "custom-model");
        assert_eq!(reloaded.message_count, 99);
    }

    #[test]
    fn test_session_not_found_errors() {
        let tmp = TempDir::new().unwrap();
        let recovery = SessionRecovery::with_dir(tmp.path().to_path_buf()).unwrap();
        let project = PathBuf::from("/tmp/nonexistent");

        // Loading non-existent session should error
        let result = recovery.load_messages(&project, "nonexistent-id");
        assert!(result.is_err(), "Should error for nonexistent session");

        // Appending to non-existent session should error
        let result = recovery.append_message(&project, "nonexistent-id", 0, &make_user_msg("test"));
        assert!(result.is_err(), "Should error appending to nonexistent session");

        // Loading metadata for non-existent session should error
        let result = recovery.load_metadata(&project, "nonexistent-id");
        assert!(result.is_err(), "Should error loading metadata for nonexistent session");
    }

    #[test]
    fn test_session_different_projects_isolation() {
        let tmp = TempDir::new().unwrap();
        let recovery = SessionRecovery::with_dir(tmp.path().to_path_buf()).unwrap();

        let project_a = PathBuf::from("/tmp/project-a");
        let project_b = PathBuf::from("/tmp/project-b");

        // Create sessions in different projects
        let id_a = recovery.create_session(&project_a, "model-a").unwrap();
        let id_b = recovery.create_session(&project_b, "model-b").unwrap();

        // Add different messages
        recovery.append_message(&project_a, &id_a, 0, &make_user_msg("project A msg")).unwrap();
        recovery.append_message(&project_b, &id_b, 0, &make_user_msg("project B msg")).unwrap();

        // Verify isolation
        let msgs_a = recovery.load_messages(&project_a, &id_a).unwrap();
        let msgs_b = recovery.load_messages(&project_b, &id_b).unwrap();

        assert_eq!(msgs_a.len(), 1);
        assert_eq!(msgs_b.len(), 1);
        assert_eq!(text_of(&msgs_a[0]), "project A msg");
        assert_eq!(text_of(&msgs_b[0]), "project B msg");

        // Cross-project access should fail
        assert!(recovery.load_messages(&project_a, &id_b).is_err());
        assert!(recovery.load_messages(&project_b, &id_a).is_err());
    }
}
