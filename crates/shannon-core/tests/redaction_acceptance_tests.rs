//! §4.14 verification standard ② — "a session containing injected secrets
//! leaves no plaintext on disk".
//!
//! Writes secrets of every source (built-in prefixes, user extra prefix,
//! user regex match, configured value, env-secret snapshot) through a real
//! [`SessionTee`] using the same code path the engine uses, then scans the
//! raw `events.jsonl` bytes: none of the injected values may survive.

use std::sync::Arc;

use serde_json::json;
use shannon_core::session_log::{RedactionPolicy, SessionTee};
use tempfile::TempDir;
use uuid::Uuid;

/// Scan fixture: every secret handed to the tee verbatim.
const SECRETS: &[&str] = &[
    // Built-in provider shapes.
    "sk-ant-injected-000111",
    "ghp_injected222333444",
    "github_pat_injectedABCDEF555",
    "xoxb-injected-666777888",
    "glpat-injected999aaa-bbb",
    // User-configured sources below (mirrored in policy_toml()).
    "xapp-1-prefixsecret",
    "internal-ticket-4242",
    "hardcoded-shared-secret",
];

fn policy_toml() -> String {
    concat!(
        "# acceptance fixture\n",
        "[prefixes]\n",
        "extra = [\"xapp-1-\"]\n",
        "\n",
        "[[patterns]]\n",
        "regex = 'internal-ticket-[0-9]{4}'\n",
        "\n",
        "[values]\n",
        "secrets = [\"hardcoded-shared-secret\"]\n"
    )
    .to_string()
}

#[test]
fn session_disk_scan_finds_no_plaintext_secrets() {
    // Register one env-shaped secret so the snapshot layer is exercised
    // exactly as it would be on a machine with KEY/SECRET/TOKEN vars set.
    const ENV_NAME: &str = "SHANNON_TEST_ACCEPTANCE_SECRET";
    const ENV_VALUE: &str = "env-leaked-secret-value";
    let saved = std::env::var_os(ENV_NAME);
    unsafe {
        std::env::set_var(ENV_NAME, ENV_VALUE);
    }

    let home = TempDir::new().expect("tempdir");
    let config_path = home.path().join("redaction.toml");
    std::fs::write(&config_path, policy_toml()).expect("write redaction.toml");
    let policy = Arc::new(RedactionPolicy::load(&config_path));

    let session_id = Uuid::new_v4().to_string();
    {
        let mut tee =
            SessionTee::open_in_dir_with_policy(home.path(), &session_id, "m", None, policy);

        tee.record_user_message(
            "prompt with sk-ant-injected-000111 and env-leaked-secret-value inside",
        );
        tee.record_turn_start(Some("q-1".into()));
        tee.record_query_event(&shannon_core::QueryEvent::Text {
            query_id: Uuid::new_v4(),
            content: "leak ghp_injected222333444 tail".to_string(),
        });
        tee.record_query_event(&shannon_core::QueryEvent::ToolUseRequest {
            query_id: Uuid::new_v4(),
            tool_use_id: "t1".into(),
            tool_name: "Bash".into(),
            tool_input: json!({"command": "echo xapp-1-prefixsecret"}),
        });
        tee.record_query_event(&shannon_core::QueryEvent::ToolUseResult {
            query_id: Uuid::new_v4(),
            tool_use_id: "t1".into(),
            tool_name: "Bash".into(),
            result:
                "out github_pat_injectedABCDEF555 xoxb-injected-666777888 glpat-injected999aaa-bbb env-leaked-secret-value"
                    .to_string(),
            is_error: false,
            meta: Box::new(json!({"note": "value hardcoded-shared-secret"})),
        });
        tee.record_request_header(
            &json!({
                "model": "m",
                "system": [{"type":"text","text":"sys contains internal-ticket-4242"}],
                "messages": [{"role":"user","content":"msg sk-ant-injected-000111"}]
            }),
            "m",
            None,
            json!({"api_key_hint": "hardcoded-shared-secret"}),
        );
        tee.close();
    }

    let path = home
        .path()
        .join("sessions")
        .join(&session_id)
        .join("events.jsonl");
    let raw = std::fs::read_to_string(&path).expect("events.jsonl on disk");

    for secret in SECRETS {
        assert!(
            !raw.contains(secret),
            "plaintext survived on disk: {secret}"
        );
    }
    assert!(
        !raw.contains("env-leaked-secret-value"),
        "env-snapshot secret survived"
    );

    // The masking happened — markers are where secrets used to be.
    let markers = raw.matches("[REDACTED]").count();
    assert!(markers >= 8, "expected multiple masks, saw {markers}");

    // And the log is still parseable L0: round-trips through the reader.
    let reader = shannon_core::session_log::SessionLogReader::open(&path).expect("reader opens");
    let events = reader
        .read_events(false)
        .expect("clean JSONL after masking");
    assert!(events.len() >= 6);

    match saved {
        Some(old) => unsafe { std::env::set_var(ENV_NAME, old) },
        None => unsafe { std::env::remove_var(ENV_NAME) },
    }
}
